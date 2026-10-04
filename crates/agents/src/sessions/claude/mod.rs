use super::*;

mod transcript;
pub(super) use transcript::parse_claude;
use transcript::parse_claude_exact;

pub(super) struct Claude;

impl AgentSessionProvider for Claude {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        environment
            .get("CLAUDE_CONFIG_DIR")
            .cloned()
            .unwrap_or_else(|| process_home.join(".claude"))
    }
    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        ClaudeSessionProvider {
            root: roots.claude.clone(),
        }
        .list_sessions(workspace_path)
    }
    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        ClaudeSessionProvider {
            root: roots.claude.clone(),
        }
        .find_session(session_id)
    }
    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        parse_claude_exact(
            &locator.transcript_path,
            &locator.trusted_root,
            &locator.session_id,
        )
        .map(|session| session.title)
    }
    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        snapshot::read_claude(locator)
    }
}

pub(crate) struct ClaudeSessionProvider {
    pub(crate) root: PathBuf,
}

impl ClaudeSessionProvider {
    pub(crate) fn list_sessions(&self, workspace_path: &Path) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        scan_claude(&self.root.join("projects"), workspace_path, &mut sessions);
        sessions
    }

    pub(crate) fn find_session(&self, session_id: &str) -> Option<AgentSession> {
        jsonl_files(&self.root.join("projects")).find_map(|path| {
            if path
                .components()
                .any(|component| component.as_os_str() == OsStr::new("subagents"))
            {
                return None;
            }
            parse_claude_exact(&path, &self.root.join("projects"), session_id)
        })
    }
}

fn scan_claude(root: &Path, worktree: &Path, sessions: &mut Vec<AgentSession>) {
    for path in jsonl_files(root) {
        if path
            .components()
            .any(|component| component.as_os_str() == OsStr::new("subagents"))
        {
            continue;
        }
        if let Some(session) = parse_claude(&path, root, worktree) {
            sessions.push(session);
        }
    }
}

fn jsonl_files(root: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension() == Some(OsStr::new("jsonl")))
        .map(|entry| entry.into_path())
}
