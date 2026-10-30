use crate::input::TaskInput;
use anyhow::{Context, Result};
use aow_terminald_client::TerminaldClient;
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{io, path::PathBuf, time::Duration};

const HELP: &str = "Task commands use the running AoW server's same-user CLI socket; terminald and browser login are not required.
A child task inherits its project, parent task, role, runtime and current workspace from --from-session-id or AOW_SESSION_ID.
Use --worktree new for an automatically prepared checkout; current (default) reuses the parent's checkout.
A path selects an existing worktree in that project. Root creation outside a Task requires --project-id.
create returns the stable task ID and native ACP session ID after the prompt is accepted, not after execution finishes.
If startup fails or times out, inspect the returned task ID with get instead of blindly creating another task.
archive/delete clean up the Task subtree's sessions and keep worktrees and files.

Examples:
  aow-cli task create --task 'Review the changes' --worktree new
  aow-cli task create --from-session-id SESSION-ID --task-file task.md
  aow-cli task list
  aow-cli task get TASK-ID
  aow-cli task submit TASK-ID --task 'Check the tests'
  aow-cli task archive TASK-ID";

#[derive(Args)]
#[command(after_help = HELP)]
pub struct TaskArgs {
    #[command(subcommand)]
    command: Command,
}
#[derive(Args)]
struct ContextArgs {
    /// Calling Task's native ACP session ID. Defaults to AOW_SESSION_ID.
    #[arg(long)]
    from_session_id: Option<String>,
    /// Root creation / project queries outside a Task; otherwise inferred from session ID.
    #[arg(long, value_parser = crate::parse_id)]
    project_id: Option<String>,
}
impl ContextArgs {
    fn resolve(self) -> Result<(Option<String>, Option<String>)> {
        let session = match self.from_session_id {
            Some(session) => Some(session),
            None if self.project_id.is_some() => None,
            None => match std::env::var("AOW_SESSION_ID") {
                Ok(session) => Some(session),
                Err(std::env::VarError::NotPresent) => None,
                Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidInput, error).into()),
            },
        };
        if session.as_ref().is_some_and(|id| id.trim().is_empty()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session ID must not be empty",
            )
            .into());
        }
        if session.is_none() && self.project_id.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "provide --from-session-id / AOW_SESSION_ID, or --project-id outside a Task",
            )
            .into());
        }
        Ok((session, self.project_id))
    }
}
#[derive(Subcommand)]
enum Command {
    /// Create and execute a task, inheriting context from the calling ACP session.
    #[command(after_help = HELP)]
    Create {
        #[command(flatten)]
        context: ContextArgs,
        #[command(flatten)]
        task: TaskInput,
        /// new, current, or an existing worktree path. Defaults to the parent's workspace.
        #[arg(long, default_value = "current")]
        worktree: String,
        /// Override the inherited ACP runtime configuration.
        #[arg(long)]
        agent_runtime_id: Option<String>,
        /// Override the automatic branch suffix; an empty value disables the suffix.
        #[arg(long)]
        slug: Option<String>,
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=600))]
        timeout: u64,
    },
    /// List project tasks with their parent relationships.
    List {
        #[command(flatten)]
        context: ContextArgs,
        #[arg(long, value_parser = crate::parse_id)]
        parent_id: Option<String>,
        #[arg(long)]
        include_archived: bool,
    },
    /// Read task details and the main ACP session state.
    Get {
        #[arg(value_parser = crate::parse_id)]
        task_id: String,
    },
    /// Append a prompt to the task's main session; returns after acceptance.
    Submit {
        #[arg(value_parser = crate::parse_id)]
        task_id: String,
        #[command(flatten)]
        task: TaskInput,
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=600))]
        timeout: u64,
    },
    /// Archive a task subtree and clean up its ACP sessions.
    Archive {
        #[arg(value_parser = crate::parse_id)]
        task_id: String,
    },
    /// Delete a task subtree and clean up its ACP sessions.
    Delete {
        #[arg(value_parser = crate::parse_id)]
        task_id: String,
    },
}
#[derive(Debug)]
pub struct StartupFailure(pub Value);
impl std::fmt::Display for StartupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Task startup did not finish; inspect the returned task ID before retrying")
    }
}
impl std::error::Error for StartupFailure {}
impl StartupFailure {
    pub fn json(&self) -> Value {
        json!({"error":{"code":"task_not_ready","message":self.to_string(),"task_id":self.0["id"]},"task":self.0})
    }
}
fn text(input: TaskInput) -> Result<String> {
    let text = input.read()?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "--task or --task-file is required",
        )
    })?;
    if text.trim().is_empty() || text.len() > 131_072 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "task must contain 1-131072 bytes",
        )
        .into());
    }
    Ok(text)
}
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
pub fn execute(args: TaskArgs, state_dir: PathBuf) -> Result<Value> {
    let cwd = std::env::current_dir()?;
    let state_dir = if state_dir.is_absolute() {
        state_dir
    } else {
        cwd.join(state_dir)
    };
    let client = TerminaldClient::new(state_dir.join("runtime/cli/cli.sock"));
    let deadline = match &args.command {
        Command::Create { timeout, .. } => timeout + 260,
        Command::Submit { timeout, .. } => timeout + 20,
        Command::Archive { .. } | Command::Delete { .. } => 600,
        _ => 20,
    };
    tokio::runtime::Runtime::new()?.block_on(async {
        tokio::time::timeout(Duration::from_secs(deadline), async {
            match args.command {
                Command::Create { context, task, worktree, agent_runtime_id, slug, timeout } => {
                    let (from_session_id, project_id) = context.resolve()?;
                    let worktree = if matches!(worktree.as_str(), "new" | "current") || PathBuf::from(&worktree).is_absolute() { worktree } else { cwd.join(worktree).to_string_lossy().into_owned() };
                    let result: Value = client.post_json("/v1/tasks", &json!({"from_session_id":from_session_id,"project_id":project_id,"task":text(task)?,"worktree":worktree,"agent_runtime_id":agent_runtime_id,"slug":slug,"request_id":aow_id::new_id(),"timeout_seconds":timeout})).await?;
                    if result["phase"] != "ready" { return Err::<Value, anyhow::Error>(StartupFailure(result).into()); }
                    Ok(result)
                },
                Command::List { context, parent_id, include_archived } => {
                    let (session, project) = context.resolve()?;
                    let mut query = vec![format!("include_archived={include_archived}")];
                    if let Some(session) = session { query.push(format!("from_session_id={}", encode(&session))); }
                    if let Some(project) = project { query.push(format!("project_id={}", encode(&project))); }
                    if let Some(parent) = parent_id { query.push(format!("parent_id={}", encode(&parent))); }
                    Ok(client.get_json(&format!("/v1/tasks?{}", query.join("&"))).await?)
                },
                Command::Get { task_id } => Ok(client.get_json(&format!("/v1/tasks/{task_id}")).await?),
                Command::Submit { task_id, task, timeout } => Ok(client.post_json(&format!("/v1/tasks/{task_id}/submit"), &json!({"task":text(task)?,"timeout_seconds":timeout})).await?),
                Command::Archive { task_id } => Ok(client.post_json(&format!("/v1/tasks/{task_id}/archive"), &json!({})).await?),
                Command::Delete { task_id } => Ok(client.post_json(&format!("/v1/tasks/{task_id}/delete"), &json!({})).await?),
            }
        }).await.context("Task request timed out; inspect task get/list before retrying")?
    }).with_context(|| format!("local Task API at {}; ensure AoW server is running with this state directory", client.socket_path().display()))
}
