use anyhow::{Context, Result};
use aow_protocol::{
    AgentTerminalCreate, AgentTerminalInfo, AgentTerminalPhase, AgentTerminalSubmit,
};
use aow_terminald_client::TerminaldClient;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    path::PathBuf,
    time::Duration,
};

const HELP: &str = "Requires a running AoW server and terminald for the same --state-dir.
Uses a same-user Unix socket, without a browser PIN or Cookie.
CLI terminals start hidden at 160 columns by 48 rows. Open Terminal in the right
sidebar to observe; take over explicitly once startup is complete.
The caller must prepare the worktree before creating an agent. --project-id and
--cwd are required; the existing worktree root must belong to that registered project.
No worktrees or branches are created, switched, or reset. Worktrees and sessions
remain after tasks finish. Output includes pane_id, tab_id and phase.
CLI-created tabs use automatic titles and cannot be renamed.
create waits until the agent's terminal UI is ready for input, not task completion.
Commands address the pane; users may change the session inside it at any time.
submit writes the task and Enter; it does not wait for an agent reply. A human
controller causes a conflict. Do not automatically retry an ambiguous submission.

Examples:
  aow-cli project list
  aow-cli project get PROJECT-ID
  aow-cli agent create --project-id PROJECT-ID --agent codex --cwd /repo
  aow-cli agent create --project-id PROJECT-ID --agent traecli --cwd /worktrees/fix --task-file task.md
  aow-cli agent submit --pane-id PANE-ID --task 'Continue with the confirmed review feedback'
  aow-cli agent get --pane-id PANE-ID
  aow-cli agent list";

#[derive(Args)]
#[command(after_help = HELP)]
pub struct AgentArgs {
    #[command(subcommand)]
    command: AgentCommand,
}

#[derive(Subcommand)]
enum AgentCommand {
    /// Create a hidden agent pane, wait until ready, optionally submit its first task.
    #[command(after_help = HELP)]
    Create {
        #[arg(long, default_value = "codex", value_parser = ["codex", "traecli", "hermes"])]
        agent: String,
        /// Registered project that owns the worktree.
        #[arg(long, value_parser = crate::parse_id)]
        project_id: String,
        /// Existing worktree root belonging to --project-id.
        #[arg(long)]
        cwd: PathBuf,
        /// Deadline in seconds for input readiness and optional first task submission (1-600).
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=600))]
        timeout: u64,
        #[command(flatten)]
        task: TaskInput,
    },
    /// Submit another task to a ready pane; fails if a user currently controls it.
    #[command(after_help = HELP)]
    Submit {
        #[arg(long, value_parser = crate::parse_id)]
        pane_id: String,
        #[command(flatten)]
        task: TaskInput,
    },
    /// Inspect startup outcome and runtime status for a pane.
    #[command(after_help = HELP)]
    Get {
        #[arg(long, value_parser = crate::parse_id)]
        pane_id: String,
    },
    /// List CLI-created agent panes, including hidden and failed panes.
    #[command(after_help = HELP)]
    List,
}

#[derive(Args)]
struct TaskInput {
    #[arg(long, conflicts_with = "task_file")]
    task: Option<String>,
    /// Read UTF-8 task text from a file, or '-' for stdin (maximum 128 KiB).
    #[arg(long, conflicts_with = "task")]
    task_file: Option<PathBuf>,
}

impl TaskInput {
    fn read(self) -> Result<Option<String>> {
        let Some(path) = self.task_file else {
            return Ok(self.task);
        };
        let mut text = String::new();
        if path.as_os_str() == "-" {
            io::stdin().take(131_073).read_to_string(&mut text)?;
        } else {
            std::fs::File::open(path)?
                .take(131_073)
                .read_to_string(&mut text)?;
        }
        if text.len() > 131_072 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "task exceeds 128 KiB").into());
        }
        Ok(Some(text))
    }
}

#[derive(Debug)]
pub struct StartupFailure(pub AgentTerminalInfo);
impl std::fmt::Display for StartupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0
                .state
                .error
                .as_deref()
                .unwrap_or("agent did not become ready")
        )
    }
}
impl std::error::Error for StartupFailure {}

pub fn execute(args: AgentArgs, state_dir: PathBuf) -> Result<Value> {
    let invocation = std::env::current_dir()?;
    let absolute = |path: PathBuf| {
        if path.is_absolute() {
            path
        } else {
            invocation.join(path)
        }
    };
    let client = TerminaldClient::new(absolute(state_dir).join("cli/cli.sock"));
    tokio::runtime::Runtime::new()?.block_on(async {
        let timeout = match &args.command { AgentCommand::Create { timeout, .. } => *timeout + 20, _ => 20 };
        tokio::time::timeout(Duration::from_secs(timeout), async {
            match args.command {
                AgentCommand::Create { agent, project_id, cwd, timeout, task } => {
                    let request = AgentTerminalCreate {
                        agent, project_id, cwd: absolute(cwd).to_string_lossy().into_owned(),
                        timeout_seconds: timeout, task: task.read()?,
                    };
                    let info: AgentTerminalInfo = client.post_json("/v1/agents", &request).await?;
                    if info.state.phase != AgentTerminalPhase::Ready { return Err::<Value, anyhow::Error>(StartupFailure(info).into()); }
                    Ok(serde_json::to_value(info)?)
                }
                AgentCommand::Submit { pane_id, task } => {
                    let task = task.read()?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "submit requires --task or --task-file"))?;
                    Ok(client.post_json::<_, Value>(&format!("/v1/agents/{pane_id}/submit"), &AgentTerminalSubmit { task }).await?)
                }
                AgentCommand::Get { pane_id } => Ok(client.get_json::<Value>(&format!("/v1/agents/{pane_id}")).await?),
                AgentCommand::List => Ok(client.get_json::<Value>("/v1/agents").await?),
            }
        }).await.context("AoW CLI request timed out; operation may still be running, inspect agent list before retrying")?
    }).with_context(|| format!("local agent API at {}; ensure AoW server and terminald are running with this state directory", client.socket_path().display()))
}

pub fn failure_json(failure: &StartupFailure) -> Value {
    json!({"error": {"code": "agent_not_ready", "message": failure.to_string(),
        "pane_id": failure.0.pane_id, "tab_id": failure.0.tab_id}, "agent": failure.0})
}
