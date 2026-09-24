use anyhow::{Context, Result};
use aow_terminald_client::{TerminaldClient, TerminaldClientError};
use clap::{Args, Subcommand};
use serde_json::Value;
use std::{io, path::PathBuf, time::Duration};

const HELP: &str = "Query registered projects through the running AOW server's local Unix socket.
Uses the same --state-dir and OS user as agent commands. No terminald is needed.
Queries only read the project registry; they do not query Git or return worktrees.
list returns {items:[...]}; get returns one project. Each project contains id, name
and repo_path (the registered repository directory).
Use Git at repo_path to discover or prepare worktrees, then pass the project
id and the chosen worktree root to agent create.

Examples:
  aow-cli project list
  aow-cli project get PROJECT-ID
  git -C /repo worktree list --porcelain
  aow-cli agent create --project-id PROJECT-ID --cwd /worktrees/fix --task-file task.md";

#[derive(Args)]
#[command(after_help = HELP)]
pub struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// List registered project IDs, names and repository paths.
    #[command(after_help = HELP)]
    List,
    /// Get a registered project's ID, name and repository path.
    #[command(after_help = HELP)]
    Get {
        #[arg(value_parser = crate::parse_id)]
        project_id: String,
    },
}

pub fn execute(args: ProjectArgs, state_dir: PathBuf) -> Result<Value> {
    let client = TerminaldClient::new(state_dir.join("cli/cli.sock"));
    let path = match args.command {
        ProjectCommand::List => "/v1/projects".to_owned(),
        ProjectCommand::Get { project_id } => format!("/v1/projects/{project_id}"),
    };
    tokio::runtime::Runtime::new()?
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(20), client.get_json::<Value>(&path))
                .await
                .map_err(|_| {
                    TerminaldClientError::Io(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "AOW project query timed out",
                    ))
                })?
        })
        .with_context(|| {
            format!(
                "local project API at {}; ensure AOW server is running with this state directory",
                client.socket_path().display()
            )
        })
}
