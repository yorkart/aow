use std::{
    io::{self, Read},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result};
use aow_terminald_client::{TerminaldClient, TerminaldClientError};
use clap::Args;
use serde_json::{Map, Value, json};

const HELP: &str =
    "Create one task from a saved task JSON object or automation get output (up to 1 MiB).
Requires a running AoW server with the same --state-dir and OS user; no terminald
or browser login is needed. --file - reads JSON from standard input.
The server assigns a new task ID, binds the task to --project-id and its repository
directory, and always sets enabled=false. Dynamic manual tasks keep their directory
unset until execution. Task settings, including workspace mode, are preserved.
Saved identity, timestamps, runtime state and Agent launch settings
are ignored; Agent launch settings are resolved on this machine.
Each successful call creates a new task. After a timeout, inspect automation list
before retrying. To migrate several files, call create once per file and continue
after individual failures.

Examples:
  aow-cli automation create --file task.json --project-id PROJECT-ID
  aow-cli automation create --file - --project-id PROJECT-ID < task.json";

#[derive(Args)]
#[command(after_help = HELP)]
pub(super) struct CreateArgs {
    /// Saved task JSON file, or - to read standard input.
    #[arg(long, value_name = "PATH")]
    file: PathBuf,
    /// Target registered project; its repository directory becomes the workspace path.
    #[arg(long, value_parser = crate::parse_id)]
    project_id: String,
}

const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub(super) fn execute(args: CreateArgs, state_dir: PathBuf) -> Result<Value> {
    let source: Box<dyn Read> = if args.file.as_os_str() == "-" {
        Box::new(io::stdin().lock())
    } else {
        Box::new(
            std::fs::File::open(&args.file)
                .with_context(|| format!("read automation file {}", args.file.display()))?,
        )
    };
    let mut bytes = Vec::new();
    source.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "automation JSON exceeds 1 MiB").into(),
        );
    }
    let configuration: Map<String, Value> = serde_json::from_slice(&bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("expected one automation JSON object: {error}"),
        )
    })?;
    let client = TerminaldClient::new(state_dir.join("cli/cli.sock"));
    let request = json!({"project_id": args.project_id, "configuration": configuration});
    tokio::runtime::Runtime::new()?.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(30),
            client.post_json::<_, Value>("/v1/automations", &request),
        ).await.map_err(|_| TerminaldClientError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            "Automation creation timed out; inspect automation list before retrying because the task may have been created",
        )))?
    }).with_context(|| format!(
        "local automation API at {}; ensure AoW server is running with this state directory",
        client.socket_path().display(),
    ))
}
