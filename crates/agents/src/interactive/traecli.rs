use super::{AgentInteractive, codex_like};

pub(super) struct TraeCli;

impl AgentInteractive for TraeCli {
    fn input_ready(&self, lines: &[String]) -> bool {
        codex_like::input_ready(lines)
    }
}
