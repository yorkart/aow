use super::*;
use crate::sessions::{AgentSessionProvider, hermes as store};

pub(super) struct Hermes;

impl AgentSessionTracker for Hermes {
    async fn resolve_live_session(&self, context: LiveSessionContext<'_>) -> SessionResolution {
        let Some(process_home) = context.environment.get("HOME") else {
            return SessionResolution::Unavailable;
        };
        let root = store::home(process_home, context.environment);
        let cwd = store::canonical_cwd(Path::new(context.cwd))
            .to_string_lossy()
            .into_owned();
        let pid = context.pid;
        tokio::task::spawn_blocking(move || resolve(&root, &cwd, pid))
            .await
            .unwrap_or(SessionResolution::Unavailable)
    }

    fn candidate_sessions(
        &self,
        target: &SessionTarget,
        cwd: &Path,
        roots: SessionRoots,
    ) -> Vec<AgentSessionLocator> {
        let SessionTarget::Id(id) = target else {
            return Vec::new();
        };
        store::Hermes
            .find_session(&roots, id)
            .filter(|session| {
                store::canonical_cwd(&session.cwd_path()) == store::canonical_cwd(cwd)
            })
            .map(|session| session.locator())
            .into_iter()
            .collect()
    }

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser> {
        Box::new(Parser)
    }

    fn completed_run_result(
        &self,
        locator: &AgentSessionLocator,
        exited_at: DateTime<Utc>,
    ) -> Result<Option<String>, SnapshotError> {
        let mut connection = store::open_db(&locator.transcript_path)?;
        let transaction = connection.transaction()?;
        let mut parser = self.task_stop_parser();
        let mut conclusion = None;
        for record in store::snapshot_records(&transaction, &locator.session_id)? {
            let timestamp = record["timestamp"]
                .as_str()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok());
            if timestamp.is_none_or(|timestamp| timestamp > exited_at) {
                return Ok(None);
            }
            // Use the same native compression lineage and completion parser as
            // live notifications, including replies archived during compaction.
            let session = record["session_id"].as_str().unwrap_or(&locator.session_id);
            if let Some(event) = parser.consume(session, &record) {
                conclusion = event.conclusion;
            }
        }
        Ok(conclusion.filter(|text| !text.trim().is_empty()))
    }
}

fn resolve(root: &Path, cwd: &str, pid: Option<i32>) -> SessionResolution {
    let Some(pid) = pid.filter(|pid| *pid > 0) else {
        return SessionResolution::NotFound;
    };
    let connection = match store::open_db(&root.join("state.db")) {
        Ok(connection) => connection,
        Err(_) if !root.join("state.db").exists() => return SessionResolution::NotFound,
        Err(_) => return SessionResolution::Unavailable,
    };
    // Hermes only writes leases when its concurrent-session cap is enabled.
    // A lease may still describe a pre-/new or pre-compression identity: verify
    // it against the current native store before accepting it.
    match std::fs::read(root.join("runtime/active_sessions.json")) {
        Ok(bytes) => {
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                return SessionResolution::Unavailable;
            };
            if let Some(entries) = value.get("entries").unwrap_or(&value).as_array() {
                let candidates: Vec<_> = entries.iter()
                    .filter(|entry| entry["pid"].as_i64() == Some(i64::from(pid)))
                    .filter(|entry| matches!(entry["surface"].as_str(), Some("cli" | "tui")))
                    .filter_map(|entry| entry["session_id"].as_str())
                    .filter(|id| connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1 AND cwd = ?2 AND ended_at IS NULL)",
                        rusqlite::params![id, cwd], |row| row.get::<_, bool>(0),
                    ).unwrap_or(false)).collect();
                if candidates.len() == 1 {
                    return SessionResolution::Resolved(SessionTarget::Id(
                        candidates[0].to_owned(),
                    ));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return SessionResolution::Unavailable,
    }
    // A new CLI has no database row until its first turn. Even a single open
    // session in this cwd can belong to another process (or a crashed one).
    // Without native PID evidence, leave association to the user's selection.
    SessionResolution::NotFound
}

struct Parser;

impl TaskStopParser for Parser {
    fn consume(&mut self, session_id: &str, record: &Value) -> Option<TaskStopped> {
        if record["session_id"] != session_id || !store::terminal_reply(record) {
            return None;
        }
        Some(TaskStopped {
            turn_id: record["id"].as_i64().map(|id| format!("hermes-{id}")),
            conclusion: store::public_text(record),
            usage: None,
        })
    }

    fn reset(&mut self) {}
}
