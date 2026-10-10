//! thread_metadata_store.rs::ThreadMetadataDb. SQL names and operation boundaries follow Zed.
use super::ThreadMetadata;
use crate::db::ThreadSafeConnection;
use anyhow::Result;
use rusqlite::{Connection, params};

#[derive(Clone)]
pub(super) struct ThreadMetadataDb(pub(crate) ThreadSafeConnection);

impl ThreadMetadataDb {
    pub(super) const NAME: &str = "ThreadMetadataDb";
    // Start at the supported baseline's current schema; upstream's historical GPUI/worktree
    // migrations don't apply to AoW. Keep its column names for subsequent backports.
    // AoW's facade uses string thread IDs and one workspace (JSON-encoded folder_paths).
    pub(super) const MIGRATIONS: &[&str] = &["CREATE TABLE sidebar_threads(
            thread_id TEXT PRIMARY KEY NOT NULL,
            session_id TEXT,
            agent_id TEXT,
            title TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            created_at TEXT,
            interacted_at TEXT,
            folder_paths TEXT,
            folder_paths_order TEXT,
            archived INTEGER DEFAULT 0,
            main_worktree_paths TEXT,
            main_worktree_paths_order TEXT,
            remote_connection TEXT,
            title_override TEXT
        ) STRICT;
        CREATE INDEX sidebar_threads_updated_at ON sidebar_threads(updated_at DESC);"];
    const LIST_QUERY: &str = "SELECT thread_id, session_id, agent_id, title, updated_at,
        created_at, interacted_at, folder_paths, archived, title_override FROM sidebar_threads ORDER BY updated_at DESC";

    pub(super) fn list(&self) -> Result<Vec<ThreadMetadata>> {
        let connection = self.0.lock().unwrap();
        let mut statement = connection.prepare(Self::LIST_QUERY)?;
        let rows = statement.query_map([], |row| {
            Ok((
                ThreadMetadata {
                    thread_id: row.get(0)?,
                    session_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    title: row.get(3)?,
                    updated_at: row.get(4)?,
                    created_at: row.get(5)?,
                    interacted_at: row.get(6)?,
                    folder_paths: Vec::new(),
                    archived: row.get(8)?,
                    title_override: row.get(9)?,
                },
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (mut metadata, paths) = row?;
            metadata.folder_paths = paths
                .map(|value| serde_json::from_str(&value))
                .transpose()?
                .unwrap_or_default();
            Ok(metadata)
        })
        .collect()
    }

    /// Accept the writer transaction so metadata and the AoW content cache commit together.
    pub(super) fn save(connection: &Connection, row: &ThreadMetadata) -> Result<()> {
        let folder_paths = serde_json::to_string(&row.folder_paths)?;
        connection.execute(
            "INSERT INTO sidebar_threads(thread_id, session_id, agent_id, title, updated_at,
            created_at, interacted_at, folder_paths, archived, title_override)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(thread_id) DO UPDATE SET
                session_id = excluded.session_id,
                agent_id = excluded.agent_id,
                title = excluded.title,
                updated_at = excluded.updated_at,
                created_at = excluded.created_at,
                interacted_at = excluded.interacted_at,
                folder_paths = excluded.folder_paths,
                archived = excluded.archived,
                title_override = excluded.title_override",
            params![
                row.thread_id,
                row.session_id,
                row.agent_id,
                row.title,
                row.updated_at,
                row.created_at,
                row.interacted_at,
                folder_paths,
                row.archived,
                row.title_override
            ],
        )?;
        Ok(())
    }

    pub(super) fn delete(connection: &Connection, thread_id: &str) -> Result<()> {
        connection.execute(
            "DELETE FROM sidebar_threads WHERE thread_id = ?",
            [thread_id],
        )?;
        Ok(())
    }
}
