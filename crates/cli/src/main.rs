mod agent;
mod project;

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::Result;
use aow_automations::{AutomationQuery, store::valid_component};
use clap::{Args, Parser, Subcommand, error::ErrorKind};
use serde::Serialize;

const CONTEXT: &str =
    "Local access: uses the current OS user and filesystem permissions. No PIN, Cookie,
token, or running AOW server is required for automation queries. Project queries
require a running server; agent commands also require terminald, via the local Unix socket.
Queries do not initialize, migrate,
repair, or clean state. Help and version work without a state directory.

State directory precedence:
  --state-dir PATH > AOW_STATE_DIR > $XDG_STATE_HOME/aow
  > $HOME/.local/state/aow (normally ~/.local/state/aow).
  Without these environment variables: the OS temp directory/aow-<pid>.
  A server started with a custom --state-dir requires the same path here.
  config.toml selects a Git repository (config-repo) and UUID directory (config-id).
  Run history stays in the local state directory. Without config.toml, queries
  read configuration in the state directory without initializing a repository.

Output: one JSON value on stdout; errors are JSON {error:{code,message}} on stderr.
Exit codes: 0 operation/help success, 1 I/O/service failure, 2 invalid arguments,
3 not found, 4 permission denied, 5 invalid state data, 6 agent not ready, 7 conflict.
A failed automation run is query data and does not make the CLI fail.
Task list returns {items:[...]}; run list returns {items:[...],next_cursor:...}.
A null next_cursor ends pagination. Get commands return one object. Run configuration
is the captured execution snapshot; task get returns the current configuration and launch.
Times use RFC 3339 UTC; unavailable values are null. No cross-file snapshot is promised.
Session IDs are returned verbatim. stdout_path/stderr_path are absolute local paths;
automation queries do not read agent sessions or output files. Paths can disappear during history cleanup.
Only the current data model is supported; no historical compatibility or migrations.

Examples (replace IDs with values returned by list commands):
  aow-cli project list
  aow-cli project get PROJECT-ID
  aow-cli agent create --project-id PROJECT-ID --cwd /worktrees/fix
  aow-cli automation list
  aow-cli automation get 12345678
  aow-cli automation runs list 12345678 --limit 20
  aow-cli automation runs get 12345678 20260913T090000000Z_1234
  aow-cli --state-dir /path/to/state automation list";

#[derive(Parser)]
#[command(name = "aow-cli", version, about = "AOW command-line interface", after_help = CONTEXT)]
struct Cli {
    /// AOW state directory; see precedence below. Never created by queries.
    #[arg(long, global = true, value_name = "PATH")]
    state_dir: Option<PathBuf>,
    /// Emit compact JSON instead of pretty-printed JSON.
    #[arg(long, global = true)]
    compact: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Query registered project IDs, names and repository paths through the local server.
    Project(project::ProjectArgs),
    /// Inspect local automation task configurations and execution records.
    #[command(after_help = CONTEXT)]
    Automation(Automation),
    /// Create hidden interactive agents and submit tasks through the local AOW server.
    Agent(agent::AgentArgs),
}

#[derive(Args)]
struct Automation {
    #[command(subcommand)]
    command: AutomationCommand,
}

#[derive(Subcommand)]
enum AutomationCommand {
    /// List all tasks, newest created first (ID descending breaks ties).
    #[command(after_help = CONTEXT)]
    List {
        /// Filter by the exact project ID.
        #[arg(long)]
        project_id: Option<String>,
        /// Include deleted task tombstones; otherwise they are hidden.
        #[arg(long)]
        include_deleted: bool,
    },
    /// Get the current saved configuration and observed status, including tombstones.
    #[command(after_help = CONTEXT)]
    Get {
        #[arg(value_parser = parse_id)]
        task_id: String,
    },
    /// Inspect execution history, including history retained for deleted tasks.
    #[command(after_help = CONTEXT)]
    Runs(Runs),
}

#[derive(Args)]
struct Runs {
    #[command(subcommand)]
    command: RunsCommand,
}

#[derive(Subcommand)]
enum RunsCommand {
    /// List runs by run ID descending; pass next_cursor as --before for the next page.
    #[command(after_help = CONTEXT)]
    List {
        #[arg(value_parser = parse_id)]
        task_id: String,
        /// Maximum number of records in this page (1-500).
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=500))]
        limit: u16,
        /// Exclusive run ID cursor from the previous page; need not still exist.
        #[arg(long, value_parser = parse_id)]
        before: Option<String>,
    },
    /// Get a run and its captured configuration, session ID, and output file paths.
    #[command(after_help = CONTEXT)]
    Get {
        #[arg(value_parser = parse_id)]
        task_id: String,
        #[arg(value_parser = parse_id)]
        run_id: String,
    },
}

fn parse_id(value: &str) -> std::result::Result<String, String> {
    valid_component(value).map_err(|error| error.to_string())?;
    Ok(value.to_owned())
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
}

#[derive(Serialize)]
struct ErrorResponse<'a> {
    error: ErrorBody<'a>,
}

fn report(code: &str, message: String, status: u8) -> ExitCode {
    let response = ErrorResponse {
        error: ErrorBody { code, message },
    };
    let _ = write_json(&mut io::stderr().lock(), &response, true);
    ExitCode::from(status)
}

fn write_json(output: &mut impl Write, value: &impl Serialize, compact: bool) -> Result<()> {
    let mut bytes = if compact {
        serde_json::to_vec(value)?
    } else {
        serde_json::to_vec_pretty(value)?
    };
    bytes.push(b'\n');
    output.write_all(&bytes)?;
    Ok(())
}

fn execute(cli: Cli) -> Result<()> {
    let state_dir = cli
        .state_dir
        .unwrap_or_else(aow_filesystem::default_state_dir);
    if let Command::Agent(args) = cli.command {
        let value = agent::execute(args, state_dir)?;
        return write_json(&mut io::stdout().lock(), &value, cli.compact);
    }
    if let Command::Project(args) = cli.command {
        let value = project::execute(args, state_dir)?;
        return write_json(&mut io::stdout().lock(), &value, cli.compact);
    }
    let query = AutomationQuery::open(state_dir)?;
    // Build a complete result before touching stdout, so failed queries never emit partial JSON.
    let value = match cli.command {
        Command::Agent(_) | Command::Project(_) => unreachable!(),
        Command::Automation(automation) => match automation.command {
            AutomationCommand::List {
                project_id,
                include_deleted,
            } => {
                serde_json::json!({ "items": query.tasks(project_id.as_deref(), include_deleted)? })
            }
            AutomationCommand::Get { task_id } => serde_json::to_value(query.task(&task_id)?)?,
            AutomationCommand::Runs(runs) => match runs.command {
                RunsCommand::List {
                    task_id,
                    limit,
                    before,
                } => {
                    serde_json::to_value(query.runs(&task_id, before.as_deref(), limit.into())?)?
                }
                RunsCommand::Get { task_id, run_id } => {
                    serde_json::to_value(query.run(&task_id, &run_id)?)?
                }
            },
        },
    };
    write_json(&mut io::stdout().lock(), &value, cli.compact)
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            return if error.print().is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        Err(error) => return report("invalid_arguments", error.to_string(), 2),
    };
    match execute(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if let Some(failure) = error.downcast_ref::<agent::StartupFailure>() {
                let _ = write_json(
                    &mut io::stderr().lock(),
                    &agent::failure_json(failure),
                    true,
                );
                return ExitCode::from(6);
            }
            if let Some(transport) =
                error.downcast_ref::<aow_terminald_client::TerminaldClientError>()
            {
                use aow_terminald_client::TerminaldClientError;
                let (code, status) = match transport {
                    TerminaldClientError::HttpStatus { status, .. } if status.as_u16() == 409 => {
                        ("conflict", 7)
                    }
                    TerminaldClientError::HttpStatus { status, .. } if status.as_u16() == 404 => {
                        ("not_found", 3)
                    }
                    TerminaldClientError::HttpStatus { status, .. } if status.as_u16() == 400 => {
                        ("invalid_arguments", 2)
                    }
                    _ => ("server_unavailable", 1),
                };
                return report(code, format!("{error:#}"), status);
            }
            let (code, status) = match error.downcast_ref::<io::Error>().map(io::Error::kind) {
                Some(io::ErrorKind::InvalidInput) => ("invalid_arguments", 2),
                Some(io::ErrorKind::NotFound) => ("not_found", 3),
                Some(io::ErrorKind::PermissionDenied) => ("permission_denied", 4),
                Some(io::ErrorKind::InvalidData) => ("invalid_data", 5),
                Some(_) => ("io_error", 1),
                None => ("invalid_data", 5),
            };
            report(code, format!("{error:#}"), status)
        }
    }
}
