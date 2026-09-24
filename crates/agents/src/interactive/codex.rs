use super::{AgentInteractive, codex_like};

pub(super) struct Codex;

impl AgentInteractive for Codex {
    fn input_ready(&self, lines: &[String]) -> bool {
        codex_like::input_ready(lines)
    }
}
