use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};

use super::model::{AgentSession, AgentSessionLocator};

const MAX_TITLE_CHARS: usize = 160;

pub(super) fn environment_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(super) fn normalize_title(value: &str) -> Option<String> {
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

pub(super) fn session(
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

pub(super) fn fallback_title(agent: &str, session_id: &str) -> String {
    format!("{agent} {}", session_id.chars().take(8).collect::<String>())
}

pub(super) fn path_is_inside_or_equal(path: &Path, root: &Path) -> bool {
    let path = normalize_path(path);
    path == root || path.starts_with(root)
}

pub(super) fn normalize_path(path: &Path) -> PathBuf {
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
