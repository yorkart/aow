use anyhow::Result;
use aow_protocol::*;
use aow_terminald_client::{TerminaldClient, TerminaldClientError};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

#[derive(Args)]
#[command(
    about = "Capture requirements and manage task board states through the local server",
    after_help = "Statuses are user-defined and shared across the board. Read statuses and the task's current revision before updating. Status changes never start or stop an agent and do not imply human approval. A conflict requires rereading the task, not blindly retrying. Conversion and start return immediately; inspect task get for execution state."
)]
pub struct TaskArgs {
    #[command(subcommand)]
    command: TaskCommand,
}
#[derive(Subcommand)]
enum TaskCommand {
    /// List board tasks (including archived tasks).
    List {
        #[arg(long, value_parser = crate::parse_id)]
        project_id: Option<String>,
    },
    /// List the board's ordered status definitions and configuration revision.
    Statuses,
    /// Get a task, execution context, revision and status history.
    Get {
        #[arg(value_parser = crate::parse_id)]
        id: String,
    },
    /// Set any status defined on the board. Does not execute the task.
    SetStatus {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Submit the initial requirement to its ready Agent once, without changing status.
    Start {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Convert an Inbox item and create its Terminal Agent. Omit --start-now to prepare only.
    Create {
        #[arg(long, value_parser = crate::parse_id)]
        inbox: String,
        #[arg(long)]
        expected_revision: u64,
        /// Request deduplication key. Reuse it after an uncertain response.
        #[arg(long, value_parser = crate::parse_id)]
        request_key: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        project_id: String,
        #[arg(long)]
        cwd: String,
        #[arg(long, default_value = "codex")]
        agent: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        start_now: bool,
        /// Create a new worktree at --cwd using this branch.
        #[arg(long, requires = "base_ref")]
        new_branch: Option<String>,
        #[arg(long, requires = "new_branch")]
        base_ref: Option<String>,
    },
    /// List a page of requirement summaries, without Markdown bodies.
    Inbox {
        #[arg(long, value_parser = crate::parse_id)]
        project_id: Option<String>,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=200))]
        limit: u16,
        #[arg(long, value_parser = parse_cursor)]
        cursor: Option<String>,
        #[arg(long)]
        include_converted: bool,
    },
    /// Read a requirement's complete Markdown body and revision.
    InboxGet {
        #[arg(value_parser = crate::parse_id)]
        id: String,
    },
    /// Quickly save a requirement without creating an Agent.
    Capture {
        #[arg(long, value_parser = crate::parse_id)]
        project_id: String,
        /// Request deduplication key; the server assigns the resource ID.
        #[arg(long, value_parser = crate::parse_id)]
        request_key: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
    },
}

pub fn execute(args: TaskArgs, state_dir: PathBuf) -> Result<Value> {
    let client = TerminaldClient::new(state_dir.join("cli/cli.sock"));
    let (path, body, collection) = match args.command {
        TaskCommand::List { project_id } => (board_path(project_id), None, Some("tasks")),
        TaskCommand::Statuses => ("/v1/tasks".into(), None, Some("statuses")),
        TaskCommand::Inbox {
            project_id,
            limit,
            cursor,
            include_converted,
        } => {
            let mut path =
                format!("/v1/tasks/inbox?limit={limit}&include_converted={include_converted}");
            if let Some(id) = project_id {
                path.push_str(&format!("&project_id={id}"));
            }
            if let Some(cursor) = cursor {
                path.push_str(&format!("&cursor={cursor}"));
            }
            (path, None, None)
        }
        TaskCommand::InboxGet { id } => (format!("/v1/tasks/inbox/{id}"), None, None),
        TaskCommand::Get { id } => (format!("/v1/tasks/items/{id}"), None, None),
        TaskCommand::SetStatus {
            id,
            status,
            expected_revision,
            reason,
        } => (
            format!("/v1/tasks/items/{id}/status"),
            Some(serde_json::to_value(TaskMove {
                expected_revision,
                status_id: status,
                reason,
            })?),
            None,
        ),
        TaskCommand::Start {
            id,
            expected_revision,
        } => (
            format!("/v1/tasks/items/{id}/start"),
            Some(json!({"expected_revision": expected_revision})),
            None,
        ),
        TaskCommand::Capture {
            project_id,
            request_key,
            title,
            description,
        } => (
            "/v1/tasks/inbox".into(),
            Some(serde_json::to_value(InboxCreate {
                project_id,
                request_key,
                title,
                description,
            })?),
            None,
        ),
        TaskCommand::Create {
            inbox,
            expected_revision,
            request_key,
            title,
            description,
            project_id,
            cwd,
            agent,
            status,
            start_now,
            new_branch,
            base_ref,
        } => (
            format!("/v1/tasks/inbox/{inbox}/convert"),
            Some(serde_json::to_value(TaskConvert {
                request_key,
                expected_revision,
                title,
                description,
                project_id,
                cwd,
                agent,
                status_id: status,
                start_now,
                worktree: new_branch.map(|branch| TaskWorktree {
                    branch,
                    base_ref: base_ref.unwrap(),
                }),
            })?),
            None,
        ),
    };
    tokio::runtime::Runtime::new()?.block_on(async {
        let result = tokio::time::timeout(Duration::from_secs(30), async {
            if let Some(body) = body {
                client.post_json::<Value, Value>(&path, &body).await
            } else {
                client.get_json::<Value>(&path).await
            }
        })
        .await
        .map_err(|_| {
            TerminaldClientError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Task API timed out; read current state before retrying a mutation",
            ))
        })??;
        Ok(if let Some(key) = collection {
            if key == "statuses" {
                json!({"items": result[key], "revision": result["status_revision"]})
            } else {
                json!({"items": result[key]})
            }
        } else {
            result
        })
    })
}

fn board_path(project_id: Option<String>) -> String {
    match project_id {
        Some(id) => format!("/v1/tasks?project_id={id}"),
        None => "/v1/tasks".into(),
    }
}

fn parse_cursor(value: &str) -> std::result::Result<String, String> {
    let valid = value.split_once('_').is_some_and(|(time, id)| {
        time.parse::<i64>().is_ok()
            && !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    });
    if valid {
        Ok(value.into())
    } else {
        Err("Invalid Inbox cursor".into())
    }
}
