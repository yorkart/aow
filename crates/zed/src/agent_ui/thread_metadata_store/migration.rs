//! AoW-only, one-time migration of JSON snapshots into the metadata/cache transaction.
use super::{ThreadMetadata, db::ThreadMetadataDb};
use crate::{
    acp_thread::{AcpThread, snapshot_store::SnapshotDb},
    db,
    facade::SessionSnapshot,
};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use std::path::Path;

const NAME: &str = "AowJsonHistory";
pub(super) fn migrate_json_history(
    connection: &mut Connection,
    state_directory: Option<&Path>,
) -> Result<()> {
    let transaction = connection.transaction()?;
    let version = db::migration_version(&transaction, NAME)?;
    ensure!(
        version <= 1,
        "ACP JSON migration is newer than this application"
    );
    if version == 1 {
        return Ok(());
    }
    if let Some(directory) = state_directory
        .map(|directory| directory.join("history"))
        .filter(|directory| directory.exists())
    {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let mut snapshot: SessionSnapshot = serde_json::from_slice(&std::fs::read(&path)?)
                .with_context(|| format!("Cannot migrate ACP history {}", path.display()))?;
            ensure!(
                path.file_stem().and_then(|name| name.to_str()) == Some(&snapshot.id),
                "ACP history filename/ID mismatch: {}",
                path.display()
            );
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM sidebar_threads WHERE thread_id = ?)",
                [&snapshot.id],
                |row| row.get(0),
            )?;
            if exists {
                continue;
            }
            snapshot.status = "disconnected".into();
            snapshot.permissions.clear();
            AcpThread::restore_history(&mut snapshot);
            ThreadMetadataDb::save(
                &transaction,
                &ThreadMetadata::from_snapshot(&snapshot, None),
            )?;
            SnapshotDb::save_thread(&transaction, &snapshot)?;
        }
    }
    // The marker shares the data transaction. Retained JSON backups can never resurrect deletions.
    db::set_migration_version(&transaction, NAME, 1)?;
    transaction.commit()?;
    Ok(())
}
