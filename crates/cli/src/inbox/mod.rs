mod input;

use anyhow::{Context, Result};
use aow_terminald_client::{TerminaldClient, TerminaldClientError};
use clap::{Args, Subcommand};
use input::TextInput;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io, path::PathBuf, time::Duration};

const HELP: &str = "Manage Inbox through the running AoW server's private Unix socket.
Uses the same OS user and --state-dir; no terminald or browser login is needed.
Agents launched by AoW inherit AOW_STATE_DIR for the originating instance.
Queries do not initialize, migrate, repair or clean files.
list returns the current Inbox snapshot; get returns one requirement.
comments list returns {items:[...]}, in append order. Comments are immutable.
create and comments add support --content or --file (- reads standard input), up to 128 KiB.
Use a new --request-key for each creation/comment and reuse it for retries.
Without --request-key each invocation uses a new key: inspect results after an ambiguous failure.
update and delete require --expected-revision from inbox get; conflicts never overwrite newer content.
Updates preserve fields not supplied. --label-id replaces labels; --clear-labels removes all labels.
delete moves the whole requirement directory into deleted/, retaining its comments.

Examples:
  aow-cli inbox list
  aow-cli inbox get REQUIREMENT-ID
  aow-cli inbox create --file requirement.md --request-key capture-one
  aow-cli inbox update REQUIREMENT-ID --expected-revision 1 --file corrected.md
  aow-cli inbox update REQUIREMENT-ID --expected-revision 2 --project-id PROJECT-ID --label-id todo
  aow-cli inbox comments list REQUIREMENT-ID
  aow-cli inbox comments add REQUIREMENT-ID --author-type ai --author Codex --file result.md --request-key result-one
  aow-cli inbox delete REQUIREMENT-ID --expected-revision 3";

#[derive(Args)]
#[command(after_help = HELP)]
pub struct InboxArgs {
    #[command(subcommand)]
    command: InboxCommand,
}

#[derive(Subcommand)]
enum InboxCommand {
    /// List active requirements, labels, comment counts and latest executions.
    #[command(after_help = HELP)]
    List,
    /// Get the current requirement, including its revision.
    #[command(after_help = HELP)]
    Get {
        #[arg(value_parser = crate::parse_id)]
        id: String,
    },
    /// Create an unbound requirement from Markdown.
    #[command(after_help = HELP)]
    Create {
        #[command(flatten)]
        text: TextInput,
        #[arg(long, value_parser = crate::parse_id)]
        request_key: Option<String>,
    },
    /// Correct Markdown, bind/unbind a project or replace labels.
    #[command(after_help = HELP)]
    Update(UpdateArgs),
    /// Move a requirement and its comments to deleted/.
    #[command(after_help = HELP)]
    Delete {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Read and append comments; editing and deletion are not supported.
    #[command(after_help = HELP)]
    Comments {
        #[command(subcommand)]
        command: CommentsCommand,
    },
}

#[derive(Args)]
struct UpdateArgs {
    #[arg(value_parser = crate::parse_id)]
    id: String,
    #[arg(long)]
    expected_revision: u64,
    #[command(flatten)]
    text: TextInput,
    #[arg(long, value_parser = crate::parse_id, conflicts_with = "unbound")]
    project_id: Option<String>,
    /// Remove the current project binding.
    #[arg(long)]
    unbound: bool,
    /// Replace the label set; repeat this flag for several labels.
    #[arg(long, value_parser = crate::parse_id, conflicts_with = "clear_labels")]
    label_id: Vec<String>,
    #[arg(long)]
    clear_labels: bool,
}

#[derive(Subcommand)]
enum CommentsCommand {
    /// Read comments in append order.
    #[command(after_help = HELP)]
    List {
        #[arg(value_parser = crate::parse_id)]
        id: String,
    },
    /// Append one immutable Markdown comment.
    #[command(after_help = HELP)]
    Add {
        #[arg(value_parser = crate::parse_id)]
        id: String,
        #[command(flatten)]
        text: TextInput,
        #[arg(long, value_parser = ["human", "ai"], default_value = "human")]
        author_type: String,
        #[arg(long)]
        author: String,
        #[arg(long, value_parser = crate::parse_id)]
        request_key: Option<String>,
    },
}

#[derive(Deserialize)]
struct CurrentItem {
    markdown: String,
    project_id: Option<String>,
    label_ids: Vec<String>,
}

enum Operation {
    Get(String),
    Comments(String),
    Post(String, Value),
    Update {
        path: String,
        revision: u64,
        markdown: Option<String>,
        project_id: Option<Option<String>>,
        labels: Option<Vec<String>>,
    },
    Delete(String, u64),
}

pub fn execute(args: InboxArgs, state_dir: PathBuf) -> Result<Value> {
    let item_path = |id: &str| format!("/v1/inbox/items/{id}");
    // Validate and read input before contacting the service.
    let operation = match args.command {
        InboxCommand::List => Operation::Get("/v1/inbox".into()),
        InboxCommand::Get { id } => Operation::Get(item_path(&id)),
        InboxCommand::Create { text, request_key } => Operation::Post(
            "/v1/inbox/items".into(),
            json!({
                "markdown": text.required()?, "request_key": request_key.unwrap_or_else(aow_id::new_id),
            }),
        ),
        InboxCommand::Update(args) => {
            let markdown = args.text.read()?;
            let project_id = if args.unbound {
                Some(None)
            } else {
                args.project_id.map(Some)
            };
            let labels = if args.clear_labels || !args.label_id.is_empty() {
                Some(args.label_id)
            } else {
                None
            };
            if markdown.is_none() && project_id.is_none() && labels.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "update requires content, a project binding or labels",
                )
                .into());
            }
            Operation::Update {
                path: item_path(&args.id),
                revision: args.expected_revision,
                markdown,
                project_id,
                labels,
            }
        }
        InboxCommand::Delete {
            id,
            expected_revision,
        } => Operation::Delete(item_path(&id), expected_revision),
        InboxCommand::Comments { command } => match command {
            CommentsCommand::List { id } => {
                Operation::Comments(format!("{}/comments", item_path(&id)))
            }
            CommentsCommand::Add {
                id,
                text,
                author_type,
                author,
                request_key,
            } => {
                if author.trim().is_empty() || author.chars().count() > 80 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "author must contain 1-80 characters",
                    )
                    .into());
                }
                Operation::Post(
                    format!("{}/comments", item_path(&id)),
                    json!({
                        "content": text.required()?, "author": { "type": author_type, "name": author },
                        "request_key": request_key.unwrap_or_else(aow_id::new_id),
                    }),
                )
            }
        },
    };
    let retry_key = match &operation {
        Operation::Post(_, body) => body["request_key"].as_str().map(str::to_owned),
        _ => None,
    };
    let client = TerminaldClient::new(state_dir.join("cli/cli.sock"));
    tokio::runtime::Runtime::new()?.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            match operation {
                Operation::Get(path) => Ok::<Value, anyhow::Error>(client.get_json::<Value>(&path).await?),
                Operation::Comments(path) => Ok(json!({ "items": client.get_json::<Vec<Value>>(&path).await? })),
                Operation::Post(path, body) => Ok(client.post_json::<_, Value>(&path, &body).await?),
                Operation::Update { path, revision, markdown, project_id, labels } => {
                    let current: CurrentItem = client.get_json(&path).await?;
                    let body = json!({
                        "expected_revision": revision,
                        "markdown": markdown.unwrap_or(current.markdown),
                        "project_id": project_id.unwrap_or(current.project_id),
                        "label_ids": labels.unwrap_or(current.label_ids),
                    });
                    Ok(client.put_json::<_, Value>(&path, &body).await?)
                }
                Operation::Delete(path, revision) => {
                    client.delete_json(&path, &json!({"expected_revision": revision})).await?;
                    Ok(json!({"id": path.rsplit('/').next().unwrap(), "deleted": true}))
                }
            }
        }).await.map_err(|_| TerminaldClientError::Io(io::Error::new(io::ErrorKind::TimedOut, "AoW Inbox request timed out; inspect the requirement and comments before retrying")))?
    }).with_context(|| {
        let retry = retry_key.map(|key| format!("; retry the same payload with --request-key {key}")).unwrap_or_default();
        format!("local Inbox API at {}; ensure AoW server is running with this state directory{retry}", client.socket_path().display())
    })
}
