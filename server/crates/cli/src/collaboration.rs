use crate::input::TaskInput;
use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::{io, path::PathBuf, time::Duration};

#[derive(Args)]
pub struct RoleArgs {
    #[command(subcommand)]
    command: RoleCommand,
}

#[derive(Subcommand)]
enum RoleCommand {
    /// List role IDs, summaries and supported collaboration runtimes.
    List,
    /// Read a role's instructions, runtime and skills on demand.
    Get {
        #[arg(value_parser = crate::parse_id)]
        id: String,
    },
}

#[derive(Args)]
#[command(about = "Delegate work from any ACP task conversation and inspect its results")]
pub struct TaskArgs {
    #[command(subcommand)]
    command: TaskCommand,
}

#[derive(Clone, ValueEnum)]
enum Intent {
    Change,
    Analyze,
    Review,
}

impl Intent {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Change => "change",
            Self::Analyze => "analyze",
            Self::Review => "review",
        }
    }
}

#[derive(Subcommand)]
enum TaskCommand {
    /// List tasks, optionally limited to the source conversation's project.
    List {
        #[arg(long, value_parser = crate::parse_id)]
        source_thread: Option<String>,
    },
    /// Read task details and its role conversations.
    Get {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[arg(long, value_parser = crate::parse_id)]
        source_thread: Option<String>,
    },
    /// Create an independent task. Reuse the request ID and payload on retries.
    #[command(
        after_help = "change creates a worktree from project defaults. analyze uses the project checkout without edits. review requires --target-task with a clean, committed workspace and records its HEAD. Without --role, use the source task's ACP adapter. Every task has a fixed main conversation, which receives AoW collaboration context separately from the supplied task text. Auxiliary sessions remain manual. After acceptance end your turn. Inbox reports require an explicit task report call; a turn ending is not a report. Do not poll or wait."
    )]
    Create {
        #[arg(long, value_parser = crate::parse_id)]
        source_thread: String,
        #[arg(long, value_parser = crate::parse_id)]
        request_id: String,
        #[arg(long, value_parser = crate::parse_id)]
        role: Option<String>,
        #[arg(long, value_enum)]
        intent: Intent,
        #[arg(long, value_parser = crate::parse_id)]
        target_task: Option<String>,
        #[command(flatten)]
        task: TaskInput,
    },
    /// Publish to Inbox and wake the parent task's main conversation by default.
    Report {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[arg(long, value_parser = crate::parse_id)]
        source_thread: String,
        #[arg(long, value_parser = crate::parse_id)]
        request_id: String,
        #[arg(long, value_parser = ["result", "progress", "blocked", "question"])]
        kind: String,
        #[arg(long, value_parser = crate::parse_id)]
        target_thread: Option<String>,
        #[command(flatten)]
        task: TaskInput,
    },
    /// Set task-wide business progress explicitly.
    Status {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[arg(value_parser = ["backlog", "todo", "in_progress", "in_review", "done", "cancelled", "blocked"])]
        status: String,
        #[arg(long, value_parser = crate::parse_id)]
        source_thread: String,
    },
    /// Queue work in the task's fixed main conversation (explicit --thread overrides it).
    Send {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[arg(long, value_parser = crate::parse_id)]
        source_thread: String,
        #[arg(long, value_parser = crate::parse_id)]
        request_id: String,
        #[arg(long, value_parser = crate::parse_id)]
        thread: Option<String>,
        #[command(flatten)]
        task: TaskInput,
    },
}

fn required_task(input: TaskInput) -> Result<String> {
    input.read()?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "--task or --task-file is required",
        )
        .into()
    })
}

fn scope(path: String, source: Option<String>) -> String {
    source.map_or_else(
        || path.clone(),
        |id| format!("{path}?source_thread_id={id}"),
    )
}

fn request(state_dir: PathBuf, path: String, body: Option<Value>) -> Result<Value> {
    let client = crate::connection::client(&state_dir)?;
    tokio::runtime::Runtime::new()?.block_on(async {
        tokio::time::timeout(Duration::from_secs(if body.is_some() { 300 } else { 20 }), async {
            match body { Some(body) => client.post_json(&path, &body).await, None => client.get_json(&path).await }
        }).await.context("AoW request timed out; inspect task list/get before retrying. For create, retain the same request ID and payload.")?
            .context("Cannot access the local AoW collaboration API")
    })
}

pub fn role(args: RoleArgs, state_dir: PathBuf) -> Result<Value> {
    request(
        state_dir,
        match args.command {
            RoleCommand::List => "/v1/roles".into(),
            RoleCommand::Get { id } => format!("/v1/roles/{id}"),
        },
        None,
    )
}

pub fn task(args: TaskArgs, state_dir: PathBuf) -> Result<Value> {
    let (path, body) = match args.command {
        TaskCommand::List { source_thread } => (scope("/v1/tasks".into(), source_thread), None),
        TaskCommand::Get { id, source_thread } => {
            (scope(format!("/v1/tasks/{id}"), source_thread), None)
        }
        TaskCommand::Create {
            source_thread,
            request_id,
            role,
            intent,
            target_task,
            task,
        } => {
            if matches!(intent, Intent::Review) != target_task.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--target-task is required only for --intent review",
                )
                .into());
            }
            (
                "/v1/tasks".into(),
                Some(
                    json!({"source_thread_id":source_thread,"request_id":request_id,
                "agent_role_id":role,"intent":intent.as_str(),"target_task_id":target_task,"prompt":required_task(task)?}),
                ),
            )
        }
        TaskCommand::Report {
            id,
            source_thread,
            request_id,
            kind,
            target_thread,
            task,
        } => (
            format!("/v1/tasks/{id}/report"),
            Some(
                json!({"request_id":request_id,"source_thread_id":source_thread,"kind":kind,"target_thread_id":target_thread,"message":required_task(task)?}),
            ),
        ),
        TaskCommand::Status {
            id,
            status,
            source_thread,
        } => (
            format!("/v1/tasks/{id}/status"),
            Some(json!({"status":status,"source_thread_id":source_thread})),
        ),
        TaskCommand::Send {
            id,
            source_thread,
            request_id,
            task,
            thread,
        } => (
            format!("/v1/tasks/{id}/send"),
            Some(
                json!({"request_id":request_id,"source_thread_id":source_thread,"thread_id":thread,"prompt":required_task(task)?}),
            ),
        ),
    };
    request(state_dir, path, body)
}
