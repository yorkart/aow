//! Durable business completion outbox. Conversation execution never depends on a live Server.
use anyhow::Result;
use aow_acp_core::TurnCompletion;
use rusqlite::OptionalExtension;
use std::{path::Path, sync::Mutex};

pub(crate) struct CompletionLog {
    connection: Mutex<rusqlite::Connection>,
    pub id: String,
}
impl CompletionLog {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let connection = rusqlite::Connection::open(root.join("completion-outbox.sqlite"))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS completions(sequence INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL);")?;
        connection
            .execute_batch("CREATE TABLE IF NOT EXISTS journal_identity(id TEXT PRIMARY KEY);")?;
        let id: Option<String> = connection
            .query_row("SELECT id FROM journal_identity LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        let id = match id {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                connection.execute("INSERT INTO journal_identity(id) VALUES(?1)", [&id])?;
                id
            }
        };
        Ok(Self {
            connection: Mutex::new(connection),
            id,
        })
    }
    pub(crate) fn append(&self, completion: &TurnCompletion) -> Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO completions(payload) VALUES(?1)",
            [serde_json::to_string(completion)?],
        )?;
        Ok(())
    }
    pub(crate) fn head(&self) -> Result<u64> {
        Ok(self.connection.lock().unwrap().query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM completions",
            [],
            |row| row.get(0),
        )?)
    }
    pub(crate) fn after(&self, after: u64) -> Result<Vec<(u64, TurnCompletion)>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare("SELECT sequence,payload FROM completions WHERE sequence > ?1 ORDER BY sequence LIMIT 128")?;
        statement
            .query_map([after], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
            })?
            .map(|row| {
                let (id, payload) = row?;
                Ok((id, serde_json::from_str(&payload)?))
            })
            .collect()
    }
}
