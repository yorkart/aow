use crate::input::{CreationArgs, TaskInput};
use anyhow::{Context, Result};
use aow_protocol::AcpCreate;
use aow_terminald_client::TerminaldClient;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{io, path::PathBuf, time::Duration};

const HELP: &str = "create requires --session-id or AOW_SESSION_ID for the calling session.
This execution context never changes the task text or the child session identity.
Requires the AoW server for the same --state-dir, without browser login or terminald.
--agent-id selects a configured Settings → ACP adapter, not a Settings → Agents profile.
--cwd must be an existing repository/worktree root of --project-id.
create starts a durable ACP thread and optionally submits its first task. Success means
the session is ready or the task was accepted, not that the agent has finished.
Use the returned thread_id for get/submit. Startup failures also return that identity;
inspect it before retrying a creation. submit resumes a disconnected session and rejects
busy sessions or sessions controlled by a browser. Ambiguous submissions must not be retried blindly.
--parent terminal:PANE-ID or --parent acp:THREAD-ID records the creating instance.
An inherited AOW_INSTANCE is used otherwise; --no-parent creates a root.
ACP callers pass their native session ID; AOW_ACP_CONNECTION_ID scopes the server lookup.
Outside an AoW caller, use --no-parent to explicitly create a root.
Use aow-cli instance get acp:THREAD-ID to trace ancestors, including deleted instances.";

#[derive(Args)]
#[command(after_help = HELP)]
pub struct AcpArgs {
    #[command(subcommand)]
    command: AcpCommand,
}

#[derive(Subcommand)]
enum AcpCommand {
    /// Create and start an ACP session, optionally submitting a first task.
    #[command(after_help = HELP)]
    Create {
        /// Saved ACP adapter ID; omitted uses its default, then the first configured adapter.
        #[arg(long)]
        agent_id: Option<String>,
        #[arg(long, value_parser = crate::parse_id)]
        project_id: String,
        #[arg(long)]
        cwd: PathBuf,
        #[command(flatten)]
        parent: CreationArgs,
        #[command(flatten)]
        task: TaskInput,
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=600))]
        timeout: u64,
    },
    /// Submit a task; returns after acceptance without waiting for the response.
    #[command(after_help = HELP)]
    Submit {
        #[arg(long, value_parser = crate::parse_id)]
        thread_id: String,
        #[command(flatten)]
        task: TaskInput,
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=600))]
        timeout: u64,
    },
    /// Read a session snapshot, including parent, transcript and pending permissions.
    Get {
        #[arg(long, value_parser = crate::parse_id)]
        thread_id: String,
    },
    /// List all saved ACP sessions, including browser-created sessions.
    List,
    /// List configured ACP adapters and their availability.
    Agents,
}

#[derive(Debug)]
pub struct StartupFailure(Value);
impl std::fmt::Display for StartupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            self.0["startup_error"]
                .as_str()
                .unwrap_or("ACP session did not become ready"),
        )
    }
}
impl std::error::Error for StartupFailure {}
impl StartupFailure {
    pub fn json(&self) -> Value {
        json!({"error":{"code":"acp_not_ready", "message":self.to_string(), "thread_id":self.0["thread_id"]}, "session":self.0})
    }
}

pub fn execute(args: AcpArgs, state_dir: PathBuf) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let absolute = |path: PathBuf| {
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    let client = TerminaldClient::new(absolute(state_dir).join("cli/cli.sock"));
    let timeout = match &args.command {
        AcpCommand::Create { timeout, .. } | AcpCommand::Submit { timeout, .. } => timeout + 20,
        _ => 20,
    };
    tokio::runtime::Runtime::new()?
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(timeout), async {
                match args.command {
                    AcpCommand::Create {
                        agent_id,
                        project_id,
                        cwd,
                        parent,
                        task,
                        timeout,
                    } => {
                        let creation = parent.resolve()?;
                        let request = AcpCreate {
                            agent_id,
                            project_id,
                            cwd: absolute(cwd).to_string_lossy().into_owned(),
                            parent: creation.parent,
                            caller: Some(creation.caller),
                            no_parent: creation.no_parent,
                            task: task.read()?,
                            timeout_seconds: timeout,
                        };
                        let info: Value = client.post_json("/v1/acp/sessions", &request).await?;
                        if info["phase"] != "ready" {
                            return Err::<Value, anyhow::Error>(StartupFailure(info).into());
                        }
                        Ok(info)
                    }
                    AcpCommand::Submit {
                        thread_id,
                        task,
                        timeout,
                    } => {
                        let task = task.read()?.ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "submit requires --task or --task-file",
                            )
                        })?;
                        Ok(client
                            .post_json(
                                &format!("/v1/acp/sessions/{thread_id}/submit"),
                                &json!({"task":task,"timeout_seconds":timeout}),
                            )
                            .await?)
                    }
                    AcpCommand::Get { thread_id } => Ok(client
                        .get_json(&format!("/v1/acp/sessions/{thread_id}"))
                        .await?),
                    AcpCommand::List => Ok(client.get_json("/v1/acp/sessions").await?),
                    AcpCommand::Agents => Ok(client.get_json("/v1/acp/agents").await?),
                }
            })
            .await
            .context("AoW CLI request timed out; inspect acp list before retrying")?
        })
        .with_context(|| {
            format!(
                "local ACP API at {}; ensure AoW server is running with this state directory",
                client.socket_path().display()
            )
        })
}
