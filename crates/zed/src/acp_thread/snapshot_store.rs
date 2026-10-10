//! AoW-only offline display cache. External ACP content is not stored by Zed's metadata DB.
//! The load_thread/save_thread/delete_thread boundary and versioned JSON blob mirror agent::db.
use crate::facade::SessionSnapshot;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub(crate) struct SnapshotDb;
#[derive(Serialize, Deserialize)]
struct DbThread {
    version: u32,
    #[serde(flatten)]
    snapshot: SessionSnapshot,
}
impl SnapshotDb {
    pub(crate) const NAME: &str = "AowAcpSnapshotDb";
    pub(crate) const MIGRATIONS: &[&str] = &[
        "CREATE TABLE acp_thread_snapshots(
            thread_id TEXT PRIMARY KEY NOT NULL REFERENCES sidebar_threads(thread_id) ON DELETE CASCADE,
            data_type TEXT NOT NULL,
            data BLOB NOT NULL
        ) STRICT;",
    ];
    pub(crate) fn save_thread(connection: &Connection, snapshot: &SessionSnapshot) -> Result<()> {
        #[derive(Serialize)]
        struct SerializedThread<'a> {
            version: u32,
            #[serde(flatten)]
            snapshot: &'a SessionSnapshot,
        }
        let data = serde_json::to_vec(&SerializedThread {
            version: 1,
            snapshot,
        })?;
        connection.execute("INSERT INTO acp_thread_snapshots(thread_id, data_type, data) VALUES (?1, 'json', ?2)
            ON CONFLICT(thread_id) DO UPDATE SET data_type = excluded.data_type, data = excluded.data", params![snapshot.id, data])?;
        Ok(())
    }
    pub(crate) fn load_thread(
        connection: &Connection,
        id: &str,
    ) -> Result<Option<SessionSnapshot>> {
        let row: Option<(String, Vec<u8>)> = connection
            .query_row(
                "SELECT data_type, data FROM acp_thread_snapshots WHERE thread_id = ? LIMIT 1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(data_type, data)| {
            ensure!(
                data_type == "json",
                "Unsupported ACP snapshot encoding: {data_type}"
            );
            let thread: DbThread = serde_json::from_slice(&data)?;
            ensure!(
                thread.version == 1,
                "Unsupported ACP snapshot version: {}",
                thread.version
            );
            ensure!(
                thread.snapshot.id == id,
                "ACP snapshot ID does not match its metadata"
            );
            Ok(thread.snapshot)
        })
        .transpose()
    }
    pub(crate) fn delete_thread(connection: &Connection, id: &str) -> Result<()> {
        connection.execute("DELETE FROM acp_thread_snapshots WHERE thread_id = ?", [id])?;
        Ok(())
    }
}
