use super::*;
use crate::sessions::{
    AgentSessionProvider, pi as store,
    usage::{PiUsage, TokenUsage, TokenUsageParser},
};

pub(super) struct Pi;

impl AgentSessionTracker for Pi {
    async fn resolve_live_session(&self, context: LiveSessionContext<'_>) -> SessionResolution {
        let Some(pid) = context.pid else {
            return SessionResolution::NotFound;
        };
        if !context.environment.contains_key("AOW_PI_BINDING") {
            // A directory or display title cannot uniquely identify a Pi process.
            // Unmanaged terminals still support explicit history selection.
            return SessionResolution::NotFound;
        }
        let Some(binding) = store::binding(context.environment) else {
            return SessionResolution::Unavailable;
        };
        let same_cwd = binding["cwd"]
            .as_str()
            .is_some_and(|cwd| canonical(Path::new(cwd)) == canonical(Path::new(context.cwd)));
        if binding["pid"].as_i64() != Some(i64::from(pid)) || !same_cwd {
            return SessionResolution::NotFound;
        }
        binding["session_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(|id| SessionResolution::Resolved(SessionTarget::Id(id.into())))
            .unwrap_or(SessionResolution::NotFound)
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
        store::Pi
            .find_session(&roots, id)
            .filter(|session| canonical(&session.cwd_path()) == canonical(cwd))
            .map(|session| session.locator())
            .into_iter()
            .collect()
    }

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser> {
        Box::new(Parser::default())
    }

    fn completed_run_result(
        &self,
        locator: &AgentSessionLocator,
        exited_at: DateTime<Utc>,
    ) -> Result<Option<String>, SnapshotError> {
        let before = std::fs::metadata(&locator.transcript_path)?;
        if DateTime::<Utc>::from(before.modified()?) > exited_at {
            return Ok(None);
        }
        let transcript = store::Transcript::read(&locator.transcript_path)?;
        if transcript.id != locator.session_id || transcript.cwd != locator.cwd {
            return Err(SnapshotError::Invalid("Pi session identity changed".into()));
        }
        let mut parser = Parser::default();
        for entry in transcript.branch()? {
            parser.consume(&locator.session_id, entry);
        }
        let after = std::fs::metadata(&locator.transcript_path)?;
        if before.len() != after.len() || before.modified()? != after.modified()? {
            return Ok(None);
        }
        // Successful print-process exit is the boundary for uninstrumented runs.
        Ok(parser.take_conclusion())
    }
}

fn canonical(path: &Path) -> std::path::PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| super::super::helpers::normalize_path(path))
}

#[derive(Default)]
struct Parser {
    turn_id: Option<String>,
    conclusion: Option<String>,
    usage: Option<TokenUsage>,
    token_events: PiUsage,
    settled: bool,
}

impl TaskStopParser for Parser {
    fn consume(&mut self, session_id: &str, record: &Value) -> Option<TaskStopped> {
        if record["type"] == "custom" && record["customType"] == "aow.pi" {
            let data = &record["data"];
            if data["session_id"] != session_id {
                return None;
            }
            match data["event"].as_str() {
                Some("bind" | "branch") => self.reset(),
                Some("settled") if !self.settled => {
                    self.settled = true;
                    // Intermediate tool responses and failed/aborted replies are never conclusions.
                    if self.conclusion.is_some() {
                        return Some(TaskStopped {
                            turn_id: self.turn_id.clone(),
                            conclusion: self.conclusion.clone(),
                            usage: self.usage.take(),
                        });
                    }
                }
                _ => {}
            }
            return None;
        }
        if record["type"] == "message" {
            let message = &record["message"];
            match message["role"].as_str() {
                Some("user") => {
                    self.reset();
                    self.turn_id = record["id"].as_str().map(str::to_owned);
                }
                Some("assistant") => {
                    self.settled = false;
                    self.conclusion =
                        if matches!(message["stopReason"].as_str(), Some("stop" | "length")) {
                            store::text_content(&message["content"])
                        } else {
                            None
                        };
                }
                Some("toolResult") => self.conclusion = None,
                _ => {}
            }
            if let Some(usage) = self.token_events.consume(message) {
                TokenUsage::accumulate(&mut self.usage, usage);
            }
        } else if let Some(usage) = self.token_events.consume(record) {
            TokenUsage::accumulate(&mut self.usage, usage);
        }
        None
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn take_conclusion(&mut self) -> Option<String> {
        self.conclusion.take()
    }
}
