use super::{AgentAutomation, SessionIdMode};

pub(super) struct Codex;

impl AgentAutomation for Codex {
    fn session_id_mode(&self) -> SessionIdMode {
        SessionIdMode::FromOutput
    }
    fn validate_arguments(&self, arguments: &[String]) -> anyhow::Result<()> {
        super::validate_common_arguments(arguments)
    }
    fn automation_arguments(
        &self,
        yolo: bool,
        _session_id: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        Ok(super::codex_like::codex_arguments(yolo))
    }
    fn session_from_line(&self, line: &[u8]) -> Option<String> {
        super::codex_like::codex_session_from_line(line)
    }
}
