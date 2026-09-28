use std::path::PathBuf;

use anyhow::Result;
use aow_automations::AutomationQuery;
use clap::{Args, Subcommand};
use serde_json::Value;

use super::CONTEXT;
use crate::parse_id;

#[derive(Args)]
pub(super) struct Automation {
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

pub(super) fn execute(command: Automation, state_dir: PathBuf) -> Result<Value> {
    let query = AutomationQuery::open(state_dir)?;
    // Build a complete result before touching stdout, so failed queries never emit partial JSON.
    let value = match command.command {
        AutomationCommand::List {
            project_id,
            include_deleted,
        } => serde_json::json!({ "items": query.tasks(project_id.as_deref(), include_deleted)? }),
        AutomationCommand::Get { task_id } => serde_json::to_value(query.task(&task_id)?)?,
        AutomationCommand::Runs(runs) => match runs.command {
            RunsCommand::List {
                task_id,
                limit,
                before,
            } => serde_json::to_value(query.runs(&task_id, before.as_deref(), limit.into())?)?,
            RunsCommand::Get { task_id, run_id } => {
                serde_json::to_value(query.run(&task_id, &run_id)?)?
            }
        },
    };
    Ok(value)
}
