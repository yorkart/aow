mod acp;
mod agent;
mod automation;
mod collaboration;
mod connection;
mod input;
mod instance;
mod project;

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::Result;
use aow_automations::store::valid_component;
use clap::{Parser, Subcommand, error::ErrorKind};
use serde::Serialize;

const CONTEXT: &str =
    "Server access: --server URL or AOW_SERVER_URL selects authenticated HTTP(S).
Read credentials from --server-token-file, AOW_SERVER_TOKEN_FILE, or AOW_SERVER_TOKEN.
Remote commands use Server business APIs, including automation queries; no shared
state directory or Unix socket is required. CLI tokens never authorize browser APIs.
Local access uses the current OS user and filesystem permissions.
Without remote configuration, commands use the same-user local socket; automation
queries can still inspect local state without starting Server.
Help and version never require a connection.

State directory precedence:
  --state-dir PATH > AOW_STATE_DIR > $HOME/.aow.
  Without these environment variables: the OS temp directory/aow-<pid>.
  A server started with a custom --state-dir requires the same path here.
  Configuration files live directly in the application home.
  Automation definitions live in automations/tasks; run history lives in state/automations.

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
  aow-cli automation create --file task.json --project-id PROJECT-ID
  aow-cli automation get 12345678
  aow-cli automation runs list 12345678 --limit 20
  aow-cli automation runs get 12345678 20260913T090000000Z_1234
  aow-cli --state-dir /path/to/state automation list";

#[derive(Parser)]
#[command(name = "aow-cli", version, about = "AoW command-line interface", after_help = CONTEXT)]
struct Cli {
    /// Server HTTP(S) base URL; overrides AOW_SERVER_URL.
    #[arg(long, global = true)]
    server: Option<String>,
    /// Private token file; otherwise AOW_SERVER_TOKEN_FILE or AOW_SERVER_TOKEN.
    #[arg(long, global = true)]
    server_token_file: Option<PathBuf>,
    /// AoW state directory; see precedence below. Never created by queries.
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
    /// Query agent roles for task collaboration.
    Role(collaboration::RoleArgs),
    /// Create, inspect and contact project tasks from any ACP task conversation.
    Task(collaboration::TaskArgs),
    /// Query registered project IDs, names and repository paths through Server.
    Project(project::ProjectArgs),
    /// Create paused automation tasks and inspect local configurations and execution records.
    #[command(after_help = CONTEXT)]
    Automation(automation::Automation),
    /// Create hidden interactive agents and submit tasks through Server.
    Agent(agent::AgentArgs),
    /// Create ACP sessions, submit tasks and inspect sessions through Server.
    Acp(acp::AcpArgs),
    /// Trace creation ancestry across terminal panes and ACP sessions.
    Instance(instance::InstanceArgs),
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
    connection::initialize(cli.server, cli.server_token_file)?;
    let state_dir = cli.state_dir.unwrap_or_else(aow_config::default_state_dir);
    let value = match cli.command {
        Command::Role(args) => collaboration::role(args, state_dir)?,
        Command::Task(args) => collaboration::task(args, state_dir)?,
        Command::Agent(args) => agent::execute(args, state_dir)?,
        Command::Acp(args) => acp::execute(args, state_dir)?,
        Command::Instance(args) => instance::execute(args, state_dir)?,
        Command::Project(args) => project::execute(args, state_dir)?,
        Command::Automation(args) => automation::execute(args, state_dir)?,
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
            if let Some(failure) = error.downcast_ref::<acp::StartupFailure>() {
                let _ = write_json(&mut io::stderr().lock(), &failure.json(), true);
                return ExitCode::from(6);
            }
            if let Some(failure) = error.downcast_ref::<agent::StartupFailure>() {
                let _ = write_json(
                    &mut io::stderr().lock(),
                    &agent::failure_json(failure),
                    true,
                );
                return ExitCode::from(6);
            }
            if let Some(transport) = error.downcast_ref::<aow_server_client::ServerClientError>() {
                use aow_server_client::ServerClientError;
                let (code, status) = match transport {
                    ServerClientError::HttpStatus { status, .. }
                        if status.as_u16() == 401 || status.as_u16() == 403 =>
                    {
                        ("permission_denied", 4)
                    }
                    ServerClientError::HttpStatus { status, .. } if status.as_u16() == 409 => {
                        ("conflict", 7)
                    }
                    ServerClientError::HttpStatus { status, .. } if status.as_u16() == 404 => {
                        ("not_found", 3)
                    }
                    ServerClientError::HttpStatus { status, .. } if status.as_u16() == 400 => {
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
