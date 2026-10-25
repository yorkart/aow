use anyhow::{Context, Result};
use aow_protocol::InstanceRef;
use aow_terminald_client::TerminaldClient;
use clap::{Args, Subcommand};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};

#[derive(Args)]
pub struct InstanceArgs {
    #[command(subcommand)]
    command: InstanceCommand,
}

#[derive(Subcommand)]
enum InstanceCommand {
    /// List the mixed instance graph, including retained creation records of deleted instances.
    List,
    /// Get an instance, its ancestors (nearest first) and direct children.
    Get { instance: InstanceRef },
}

pub fn execute(args: InstanceArgs, state_dir: PathBuf) -> Result<Value> {
    let client = TerminaldClient::new(
        std::env::current_dir()?
            .join(state_dir)
            .join("runtime/cli/cli.sock"),
    );
    let path = match args.command {
        InstanceCommand::List => "/v1/instances".to_owned(),
        InstanceCommand::Get { instance } => format!("/v1/instances/{instance}"),
    };
    tokio::runtime::Runtime::new()?.block_on(async {
        tokio::time::timeout(Duration::from_secs(20), client.get_json(&path))
            .await
            .context("instance query timed out")?
            .context("cannot query local AoW instance graph")
    })
}
