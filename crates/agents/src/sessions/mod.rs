mod claude;
mod codex;
mod codex_like;
mod hermes;
pub mod snapshot;
pub mod tail;
pub mod titles;
pub mod tracking;
mod traecli;

use std::{
    ffi::OsStr,
    fs::File,
    io::{BufRead, BufReader},
    path::{Component, Path, PathBuf},
};

use crate::{Agent, AgentDefinition, CLAUDE};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Map, Value};
use walkdir::WalkDir;

const MAX_TITLE_CHARS: usize = 160;
pub(super) const STATE_DB_FILENAME: &str = "state_5.sqlite";
const SQLITE_BUSY_TIMEOUT_MS: u64 = 250;

/// Reading an agent's native session history and public transcript snapshots.
pub trait AgentSessionProvider {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf;
    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession>;
    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession>;
    /// Read the current display name of an already bound session from its original
    /// store. Unavailable metadata must not invalidate the transcript subscription.
    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String>;
    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSession {
    id: String,
    agent: &'static str,
    session_id: String,
    title: String,
    cwd: String,
    created_at: String,
    updated_at: String,
    #[serde(skip)]
    transcript_path: PathBuf,
    #[serde(skip)]
    trusted_root: PathBuf,
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
        session(locator, created_at, updated_at)
    }
}

pub type SessionEnvironment = std::collections::BTreeMap<String, PathBuf>;

#[derive(Debug, Clone)]
pub struct SessionRoots {
    pub(crate) claude: PathBuf,
    pub(crate) codex: PathBuf,
    pub(crate) traecli: PathBuf,
    pub(crate) hermes: PathBuf,
}

impl SessionRoots {
    pub fn from_environment(process_home: &Path) -> Self {
        let environment = crate::KNOWN_AGENTS
            .iter()
            .flat_map(|agent| agent.definition().configuration_env)
            .filter_map(|key| environment_path(key).map(|path| ((*key).to_owned(), path)))
            .collect();
        Self::from_configuration(process_home, &environment)
    }

    pub fn from_configuration(process_home: &Path, environment: &SessionEnvironment) -> Self {
        Self {
            claude: SessionAgent::Claude.session_root(process_home, environment),
            codex: SessionAgent::Codex.session_root(process_home, environment),
            traecli: SessionAgent::TraeCli.session_root(process_home, environment),
            hermes: SessionAgent::Hermes.session_root(process_home, environment),
        }
    }

    #[cfg(test)]
    fn from_values(
        process_home: &Path,
        claude: Option<PathBuf>,
        codex: Option<PathBuf>,
        traecli: Option<PathBuf>,
        trae: Option<PathBuf>,
    ) -> Self {
        let environment = [
            ("CLAUDE_CONFIG_DIR", claude),
            ("CODEX_HOME", codex),
            ("TRAECLI_HOME", traecli),
            ("TRAE_HOME", trae),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value)))
        .collect();
        Self::from_configuration(process_home, &environment)
    }
}

pub fn list_sessions(
    workspace_path: &Path,
    agent: Option<&str>,
    roots: SessionRoots,
) -> Vec<AgentSession> {
    let workspace_path = normalize_path(workspace_path);
    let mut sessions = Vec::new();
    for provider in crate::KNOWN_AGENTS
        .iter()
        .copied()
        .filter(|candidate| agent.is_none_or(|id| id == candidate.id()))
        .filter_map(Agent::sessions)
    {
        sessions.extend(provider.list_sessions(&roots, &workspace_path));
    }
    sort_sessions(&mut sessions);
    sessions
}

/// Looks up a session using the provider-specific durable identity. In
/// particular, Codex and TraeCode CLI query their state SQLite `threads.id` primary
/// key instead of scanning the rollout tree.
pub fn find_session(agent: &str, session_id: &str, roots: SessionRoots) -> Option<AgentSession> {
    Agent::from_id(agent)?
        .sessions()?
        .find_session(&roots, session_id)
}

fn sort_sessions(sessions: &mut [AgentSession]) {
    sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn environment_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn normalize_title(value: &str) -> Option<String> {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let mut title = collapsed.chars().take(MAX_TITLE_CHARS).collect::<String>();
    if collapsed.chars().count() > MAX_TITLE_CHARS {
        title.push('…');
    }
    Some(title)
}

fn session(
    locator: AgentSessionLocator,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
) -> AgentSession {
    AgentSession {
        id: format!("{}:{}", locator.agent, locator.session_id),
        agent: locator.agent,
        session_id: locator.session_id,
        title: locator.title,
        cwd: locator.cwd.to_string_lossy().into_owned(),
        created_at: created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        updated_at: updated_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        transcript_path: locator.transcript_path,
        trusted_root: locator.trusted_root,
    }
}

fn fallback_title(agent: &str, session_id: &str) -> String {
    format!("{agent} {}", session_id.chars().take(8).collect::<String>())
}

fn path_is_inside_or_equal(path: &Path, root: &Path) -> bool {
    let path = normalize_path(path);
    path == root || path.starts_with(root)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests;

/// A built-in adapter with native session support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAgent {
    Claude,
    Codex,
    TraeCli,
    Hermes,
}

impl SessionAgent {
    pub fn agent(self) -> Agent {
        match self {
            Self::Claude => Agent::Claude,
            Self::Codex => Agent::Codex,
            Self::TraeCli => Agent::TraeCli,
            Self::Hermes => Agent::Hermes,
        }
    }
}

impl Agent {
    pub fn sessions(self) -> Option<SessionAgent> {
        match self {
            Self::Claude => Some(SessionAgent::Claude),
            Self::Codex => Some(SessionAgent::Codex),
            Self::TraeCli => Some(SessionAgent::TraeCli),
            Self::Hermes => Some(SessionAgent::Hermes),
            _ => None,
        }
    }
}

impl AgentSessionProvider for SessionAgent {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        match self {
            Self::Claude => claude::Claude.session_root(process_home, environment),
            Self::Codex => codex::Codex.session_root(process_home, environment),
            Self::TraeCli => traecli::TraeCli.session_root(process_home, environment),
            Self::Hermes => hermes::Hermes.session_root(process_home, environment),
        }
    }
    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        match self {
            Self::Claude => claude::Claude.list_sessions(roots, workspace_path),
            Self::Codex => codex::Codex.list_sessions(roots, workspace_path),
            Self::TraeCli => traecli::TraeCli.list_sessions(roots, workspace_path),
            Self::Hermes => hermes::Hermes.list_sessions(roots, workspace_path),
        }
    }
    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        match self {
            Self::Claude => claude::Claude.find_session(roots, session_id),
            Self::Codex => codex::Codex.find_session(roots, session_id),
            Self::TraeCli => traecli::TraeCli.find_session(roots, session_id),
            Self::Hermes => hermes::Hermes.find_session(roots, session_id),
        }
    }
    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        if locator.agent != self.agent().id() {
            return None;
        }
        snapshot::validate_locator(locator).ok()?;
        match self {
            Self::Claude => claude::Claude.current_title(locator),
            Self::Codex => codex::Codex.current_title(locator),
            Self::TraeCli => traecli::TraeCli.current_title(locator),
            Self::Hermes => hermes::Hermes.current_title(locator),
        }
    }
    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        if locator.agent != self.agent().id() {
            return Err(snapshot::SnapshotError::Invalid(
                "session identity does not match its agent adapter".to_owned(),
            ));
        }
        match self {
            Self::Claude => claude::Claude.read_snapshot(locator),
            Self::Codex => codex::Codex.read_snapshot(locator),
            Self::TraeCli => traecli::TraeCli.read_snapshot(locator),
            Self::Hermes => hermes::Hermes.read_snapshot(locator),
        }
    }
}
