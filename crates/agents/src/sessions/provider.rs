use super::*;

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

/// A built-in adapter with native session support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAgent {
    Claude,
    Codex,
    TraeCli,
    Hermes,
    Pi,
}

impl SessionAgent {
    pub fn agent(self) -> Agent {
        match self {
            Self::Claude => Agent::Claude,
            Self::Codex => Agent::Codex,
            Self::TraeCli => Agent::TraeCli,
            Self::Hermes => Agent::Hermes,
            Self::Pi => Agent::Pi,
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
            Self::Pi => Some(SessionAgent::Pi),
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
            Self::Pi => pi::Pi.session_root(process_home, environment),
        }
    }
    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        match self {
            Self::Claude => claude::Claude.list_sessions(roots, workspace_path),
            Self::Codex => codex::Codex.list_sessions(roots, workspace_path),
            Self::TraeCli => traecli::TraeCli.list_sessions(roots, workspace_path),
            Self::Hermes => hermes::Hermes.list_sessions(roots, workspace_path),
            Self::Pi => pi::Pi.list_sessions(roots, workspace_path),
        }
    }
    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        match self {
            Self::Claude => claude::Claude.find_session(roots, session_id),
            Self::Codex => codex::Codex.find_session(roots, session_id),
            Self::TraeCli => traecli::TraeCli.find_session(roots, session_id),
            Self::Hermes => hermes::Hermes.find_session(roots, session_id),
            Self::Pi => pi::Pi.find_session(roots, session_id),
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
            Self::Pi => pi::Pi.current_title(locator),
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
            Self::Pi => pi::Pi.read_snapshot(locator),
        }
    }
}
