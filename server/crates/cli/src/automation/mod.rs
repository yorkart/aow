use std::path::PathBuf;

use anyhow::Result;
use aow_automations::{AutomationQuery, store::RunCursor};
use clap::{Args, Subcommand};
use serde_json::Value;

use super::CONTEXT;
use crate::parse_id;

mod create;

#[derive(Args)]
pub(super) struct Automation {
    #[command(subcommand)]
    command: AutomationCommand,
}

#[derive(Subcommand)]
enum AutomationCommand {
    /// Create a new paused task from a saved JSON file in the target project.
    Create(create::CreateArgs),
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
    /// List runs newest started first; pass next_cursor as --before for the next page.
    #[command(after_help = CONTEXT)]
    List {
        #[arg(value_parser = parse_id)]
        task_id: String,
        /// Maximum number of records in this page (1-500).
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=500))]
        limit: u16,
        /// Exclusive cursor from the previous page; its record need not still exist.
        #[arg(long, value_parser = parse_run_cursor)]
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

fn parse_run_cursor(value: &str) -> std::result::Result<String, String> {
    value
        .parse::<RunCursor>()
        .map_err(|_| aow_i18n::text("automations.invalid_run_pagination_cursor").to_owned())?;
    Ok(value.to_owned())
}

pub(super) fn execute(command: Automation, state_dir: PathBuf) -> Result<Value> {
    if crate::connection::client(&state_dir)?.is_remote() {
        let client = crate::connection::client(&state_dir)?;
        let path = match command.command {
            AutomationCommand::Create(args) => return create::execute(args, state_dir),
            AutomationCommand::List {
                project_id,
                include_deleted,
            } => {
                let mut url = reqwest::Url::parse("http://placeholder/v1/automations")?;
                {
                    let mut query = url.query_pairs_mut();
                    query.append_pair("include_deleted", &include_deleted.to_string());
                    if let Some(project_id) = project_id {
                        query.append_pair("project_id", &project_id);
                    }
                }
                format!("{}?{}", url.path(), url.query().unwrap_or(""))
            }
            AutomationCommand::Get { task_id } => format!("/v1/automations/{task_id}"),
            AutomationCommand::Runs(runs) => match runs.command {
                RunsCommand::Get { task_id, run_id } => {
                    format!("/v1/automations/{task_id}/runs/{run_id}")
                }
                RunsCommand::List {
                    task_id,
                    limit,
                    before,
                } => {
                    let mut url = reqwest::Url::parse(&format!(
                        "http://placeholder/v1/automations/{task_id}/runs"
                    ))?;
                    {
                        let mut query = url.query_pairs_mut();
                        query.append_pair("limit", &limit.to_string());
                        if let Some(before) = before {
                            query.append_pair("before", &before);
                        }
                    }
                    format!("{}?{}", url.path(), url.query().unwrap_or(""))
                }
            },
        };
        return tokio::runtime::Runtime::new()?.block_on(async {
            Ok(
                tokio::time::timeout(std::time::Duration::from_secs(30), client.get_json(&path))
                    .await??,
            )
        });
    }
    // Build a complete result before touching stdout, so failed queries never emit partial JSON.
    let value = match command.command {
        AutomationCommand::Create(args) => return create::execute(args, state_dir),
        AutomationCommand::List {
            project_id,
            include_deleted,
        } => {
            serde_json::json!({ "items": AutomationQuery::open(state_dir)?.tasks(project_id.as_deref(), include_deleted)? })
        }
        AutomationCommand::Get { task_id } => {
            serde_json::to_value(AutomationQuery::open(state_dir)?.task(&task_id)?)?
        }
        AutomationCommand::Runs(runs) => {
            let query = AutomationQuery::open(state_dir)?;
            match runs.command {
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
            }
        }
    };
    Ok(value)
}
