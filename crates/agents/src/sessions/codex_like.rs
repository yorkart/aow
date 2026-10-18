use super::*;
use aow_db::codex_like::{CodexLikeDb, ThreadRecord};

pub(crate) struct CodexLikeSessionProvider {
    pub(crate) definition: &'static AgentDefinition,
    pub(crate) home: PathBuf,
}

pub(super) fn current_title(
    definition: &'static AgentDefinition,
    locator: &AgentSessionLocator,
) -> Option<String> {
    // Locators retain <configured home>/sessions, even after the agent exits or
    // the server's environment changes. Never fall back to another profile.
    let session = CodexLikeSessionProvider {
        definition,
        home: locator.trusted_root.parent()?.to_path_buf(),
    }
    .find_session(&locator.session_id)?;
    (session.transcript_path == locator.transcript_path).then_some(session.title)
}

impl CodexLikeSessionProvider {
    pub(crate) fn list_sessions(&self, workspace_path: &Path) -> Vec<AgentSession> {
        let Ok(db) = CodexLikeDb::open(&self.home.join(STATE_DB_FILENAME)) else {
            return Vec::new();
        };
        let Ok(rows) = db.list_threads(&workspace_path.to_string_lossy()) else {
            return Vec::new();
        };
        let mut sessions: Vec<_> = rows
            .into_iter()
            .map(|row| Self::row_session(self.definition, &self.home, row))
            .collect();
        if !db.has_thread_names() {
            apply_index_names(&self.home, &mut sessions);
        }
        sessions
    }

    pub(crate) fn find_session(&self, session_id: &str) -> Option<AgentSession> {
        self.find_sessions([session_id]).ok()?.into_iter().next()
    }

    pub(crate) fn find_sessions<'a>(
        &self,
        session_ids: impl IntoIterator<Item = &'a str>,
    ) -> anyhow::Result<Vec<AgentSession>> {
        let db = CodexLikeDb::open(&self.home.join(STATE_DB_FILENAME))?;
        let mut sessions: Vec<_> = db
            .find_threads(session_ids)?
            .into_iter()
            .map(|row| Self::row_session(self.definition, &self.home, row))
            .collect();
        if !db.has_thread_names() {
            apply_index_names(&self.home, &mut sessions);
        }
        Ok(sessions)
    }
}

fn apply_index_names(home: &Path, sessions: &mut [AgentSession]) {
    let Ok(file) = File::open(home.join("session_index.jsonl")) else {
        return;
    };
    let positions: std::collections::HashMap<_, _> = sessions
        .iter()
        .enumerate()
        .map(|(index, session)| (session.session_id.clone(), index))
        .collect();
    // The index is append-only. Last complete entry wins; a partially written
    // trailing record must not hide the previous name.
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(index) = entry
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| positions.get(id))
            && let Some(title) = entry
                .get("thread_name")
                .and_then(Value::as_str)
                .and_then(normalize_title)
        {
            sessions[*index].title = title;
        }
    }
}

impl CodexLikeSessionProvider {
    fn row_session(
        definition: &'static AgentDefinition,
        home: &Path,
        row: ThreadRecord,
    ) -> AgentSession {
        let session_id = row.id;
        let transcript_path = PathBuf::from(row.rollout_path);
        let cwd = PathBuf::from(row.cwd);
        let title = normalize_title(&row.title)
            .unwrap_or_else(|| fallback_title(definition.display_name, &session_id));
        let created_at = sqlite_timestamp(row.created_at);
        let updated_at = sqlite_timestamp(row.updated_at);
        session(
            AgentSessionLocator {
                agent: definition.id,
                session_id,
                title,
                cwd,
                transcript_path,
                trusted_root: home.join("sessions"),
            },
            created_at,
            updated_at,
        )
    }
}

pub(super) fn sqlite_timestamp(epoch: i64) -> DateTime<Utc> {
    let (seconds, nanos) = if epoch.abs() >= 100_000_000_000 {
        (
            epoch / 1_000,
            ((epoch % 1_000).unsigned_abs() * 1_000_000) as u32,
        )
    } else {
        (epoch, 0)
    };
    DateTime::<Utc>::from_timestamp(seconds, nanos).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}
