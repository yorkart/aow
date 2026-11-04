use super::{
    SessionService,
    model::{SavedLocator, SessionReference},
};
use anyhow::{Context, Result, ensure};
use aow_agents::sessions::{AgentSession, AgentSessionLocator};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

pub(super) fn open(root: &Path) -> Result<Connection> {
    let directory = root.join("data/runtime-daemon");
    std::fs::create_dir_all(&directory)?;
    let connection = Connection::open(directory.join("native-sessions.sqlite"))?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS native_sessions(handle TEXT PRIMARY KEY, identity TEXT UNIQUE NOT NULL, locator TEXT NOT NULL);")?;
    Ok(connection)
}
impl SessionService {
    pub(crate) fn remember(&self, session: &AgentSession) -> Result<serde_json::Value> {
        let locator = session.locator();
        let saved = SavedLocator::from(&locator);
        // Display-title changes must not allocate a new identity.
        let identity = serde_json::to_string(&(
            locator.agent,
            &locator.session_id,
            &locator.cwd,
            &locator.transcript_path,
            &locator.trusted_root,
        ))?;
        let connection = self
            .inner
            .catalog
            .lock()
            .map_err(|_| anyhow::anyhow!("Session catalog lock poisoned"))?;
        let proposed = uuid::Uuid::new_v4().to_string();
        let mut record = serde_json::to_value(&saved)?;
        record["summary"] = serde_json::to_value(session)?;
        connection.execute("INSERT INTO native_sessions(handle,identity,locator) VALUES(?1,?2,?3) ON CONFLICT(identity) DO UPDATE SET locator=excluded.locator",
            rusqlite::params![proposed,identity,serde_json::to_string(&record)?])?;
        let handle: String = connection.query_row(
            "SELECT handle FROM native_sessions WHERE identity=?1",
            [identity],
            |row| row.get(0),
        )?;
        let reference = SessionReference {
            node_id: self.inner.node_id.clone(),
            handle,
            agent: locator.agent.into(),
            session_id: locator.session_id,
            cwd: locator.cwd,
            title: locator.title,
        };
        let mut value = serde_json::to_value(session)?;
        value["reference"] = serde_json::to_value(reference)?;
        let record: aow_runtime_contract::NativeSessionRecord = serde_json::from_value(value)?;
        Ok(serde_json::to_value(record)?)
    }
    pub(super) fn locate(&self, reference: &SessionReference) -> Result<AgentSessionLocator> {
        ensure!(
            reference.node_id == self.inner.node_id,
            "Session belongs to another runtime node"
        );
        let connection = self
            .inner
            .catalog
            .lock()
            .map_err(|_| anyhow::anyhow!("Session catalog lock poisoned"))?;
        let saved: String = connection
            .query_row(
                "SELECT locator FROM native_sessions WHERE handle=?1",
                [&reference.handle],
                |row| row.get(0),
            )
            .optional()?
            .context("Native session reference is unavailable")?;
        let locator = serde_json::from_str::<SavedLocator>(&saved)?.into_locator()?;
        ensure!(
            locator.agent == reference.agent
                && locator.session_id == reference.session_id
                && locator.cwd == reference.cwd,
            "Native session reference does not match its identity"
        );
        Ok(locator)
    }
}

impl SessionService {
    pub(super) fn metadata(&self, reference: &SessionReference) -> Result<serde_json::Value> {
        self.locate(reference)?;
        let connection = self.inner.catalog.lock().unwrap();
        let record: String = connection.query_row(
            "SELECT locator FROM native_sessions WHERE handle=?1",
            [&reference.handle],
            |row| row.get(0),
        )?;
        let record: serde_json::Value = serde_json::from_str(&record)?;
        let mut value = record["summary"].clone();
        ensure!(value.is_object(), "Session metadata is unavailable");
        value["reference"] = serde_json::to_value(reference)?;
        let record: aow_runtime_contract::NativeSessionRecord = serde_json::from_value(value)?;
        Ok(serde_json::to_value(record)?)
    }
}
