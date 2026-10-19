//! Versioned AoW offline display cache. ACP owns serialization and session validation.
use crate::facade::SessionSnapshot;
use anyhow::{Result, ensure};
use aow_db::acp::SnapshotRecord;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct DbThread {
    version: u32,
    #[serde(flatten)]
    snapshot: SessionSnapshot,
}

pub(crate) fn encode(snapshot: &SessionSnapshot) -> Result<Vec<u8>> {
    #[derive(Serialize)]
    struct SerializedThread<'a> {
        version: u32,
        #[serde(flatten)]
        snapshot: &'a SessionSnapshot,
    }
    Ok(serde_json::to_vec(&SerializedThread {
        version: 1,
        snapshot,
    })?)
}

pub(crate) fn decode(id: &str, row: SnapshotRecord) -> Result<SessionSnapshot> {
    ensure!(
        row.data_type == "json",
        "Unsupported ACP snapshot encoding: {}",
        row.data_type
    );
    let thread: DbThread = serde_json::from_slice(&row.data)?;
    ensure!(
        thread.version == 1,
        "Unsupported ACP snapshot version: {}",
        thread.version
    );
    ensure!(
        thread.snapshot.thread_id == id,
        "ACP snapshot ID does not match its metadata"
    );
    Ok(thread.snapshot)
}
