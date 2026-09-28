//! Hermes 0.18 native history. Connections are read-only, including WAL reads;
//! no Hermes Python imports, schema migrations, hooks or exports are needed.
use super::*;

mod messages;

pub(super) struct Hermes;

impl AgentSessionProvider for Hermes {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        home(process_home, environment)
    }

    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        query_sessions(&roots.hermes, None, Some(workspace_path)).unwrap_or_default()
    }

    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        query_sessions(&roots.hermes, Some(session_id), None)
            .ok()?
            .pop()
    }

    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        query_sessions(&locator.trusted_root, Some(&locator.session_id), None)
            .ok()?
            .pop()
            .map(|session| session.title)
    }

    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        snapshot::read_hermes(locator)
    }
}

pub(super) fn home(process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
    let default = process_home.join(".hermes");
    let configured = environment
        .get("HERMES_HOME")
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(&default);
    // Hermes trusts an explicit profile, but still follows active_profile when
    // HERMES_HOME names the root (including custom roots outside ~/.hermes).
    if configured.parent().and_then(Path::file_name) == Some(OsStr::new("profiles")) {
        return configured.clone();
    }
    let root = if canonical_cwd(configured).starts_with(canonical_cwd(&default)) {
        &default
    } else {
        configured
    };
    if let Ok(profile) = std::fs::read_to_string(root.join("active_profile")) {
        let profile = profile.trim().to_ascii_lowercase();
        if profile != "default"
            && !profile.is_empty()
            && profile.len() <= 64
            && profile.as_bytes()[0].is_ascii_alphanumeric()
            && profile
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b"_-".contains(&ch))
        {
            return root.join("profiles").join(profile);
        }
    }
    configured.clone()
}

mod content;
mod database;
mod history;
mod sessions;

use content::content_text;
pub(super) use database::{has_column, open_db, timestamp};
pub(super) use history::{
    compacted_records, history, public_text, record, snapshot_records, terminal_reply,
};
pub(super) use sessions::canonical_cwd;
use sessions::query_sessions;

#[cfg(test)]
mod tests;
