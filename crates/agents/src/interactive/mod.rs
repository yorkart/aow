//! Interactive terminal capability and built-in adapter dispatch.

mod codex;
mod codex_like;
mod hermes;
mod traecli;

use crate::Agent;

/// Agent-specific startup readiness. Inputs are current VT
/// viewport lines, never accumulated raw PTY output or scrollback.
pub trait AgentInteractive: Send + Sync {
    /// Whether the current screen indicates the agent can accept input.
    fn input_ready(&self, lines: &[String]) -> bool;
}

/// Only agents with a complete interactive startup adapter are available here.
#[derive(Clone, Copy, Debug)]
pub enum InteractiveAgent {
    Codex,
    TraeCli,
    Hermes,
}

impl Agent {
    pub fn interactive(self) -> Option<InteractiveAgent> {
        match self {
            Self::Codex => Some(InteractiveAgent::Codex),
            Self::TraeCli => Some(InteractiveAgent::TraeCli),
            Self::Hermes => Some(InteractiveAgent::Hermes),
            _ => None,
        }
    }
}

impl AgentInteractive for InteractiveAgent {
    fn input_ready(&self, lines: &[String]) -> bool {
        match self {
            Self::Codex => codex::Codex.input_ready(lines),
            Self::TraeCli => traecli::TraeCli.input_ready(lines),
            Self::Hermes => hermes::Hermes.input_ready(lines),
        }
    }
}

#[cfg(test)]
mod tests;
