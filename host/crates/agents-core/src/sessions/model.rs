use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Reading an agent's native session history and public transcript snapshots.
pub trait AgentSessionProvider {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf;
    fn list_sessions(
        &self,
        roots: &super::roots::SessionRoots,
        workspace_path: &Path,
    ) -> Vec<AgentSession>;
    fn find_session(
        &self,
        roots: &super::roots::SessionRoots,
        session_id: &str,
    ) -> Option<AgentSession>;
    /// Read the current display name of an already bound session from its original
    /// store. Unavailable metadata must not invalidate the transcript subscription.
    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String>;
    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<super::snapshot::AgentSessionSnapshot, super::snapshot::SnapshotError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSession {
    pub(super) id: String,
    pub(super) agent: &'static str,
    pub(super) session_id: String,
    pub(super) title: String,
    pub(super) cwd: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    #[serde(skip)]
    pub(super) transcript_path: PathBuf,
    #[serde(skip)]
    pub(super) trusted_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AgentSessionLocator {
    pub agent: &'static str,
    pub session_id: String,
    pub title: String,
    pub cwd: PathBuf,
    /// Native transcript file, or the Hermes state database.
    pub transcript_path: PathBuf,
    pub trusted_root: PathBuf,
}

impl AgentSession {
    pub fn cwd_path(&self) -> PathBuf {
        PathBuf::from(&self.cwd)
    }

    pub fn locator(&self) -> AgentSessionLocator {
        AgentSessionLocator {
            agent: self.agent,
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            cwd: PathBuf::from(&self.cwd),
            transcript_path: self.transcript_path.clone(),
            trusted_root: self.trusted_root.clone(),
        }
    }

    pub fn new(
        locator: AgentSessionLocator,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        super::helpers::session(locator, created_at, updated_at)
    }
}

pub type SessionEnvironment = std::collections::BTreeMap<String, PathBuf>;
