use anyhow::Result;
use rusqlite::Connection;

pub(super) fn apply(connection: &mut Connection) -> Result<()> {
    // Preserve existing domain names, version counts and order for installed databases.
    crate::sqlite::migrate(
        connection,
        "ThreadMetadataDb",
        &[
            include_str!("sidebar_threads/001_create_sidebar_threads.sql"),
            include_str!("sidebar_threads/002_add_parent.sql"),
            include_str!("sidebar_threads/003_add_source.sql"),
        ],
    )?;
    crate::sqlite::migrate(
        connection,
        "AowAcpSnapshotDb",
        &[include_str!(
            "acp_thread_snapshots/001_create_snapshots.sql"
        )],
    )
}
