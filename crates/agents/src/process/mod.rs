//! Process recognition capability, input normalization and built-in agent dispatch.

mod claude;
mod codex;
mod info;
mod traecli;

pub use info::ProcessInfo;

use crate::{Agent, KNOWN_AGENTS};

/// Matches one normalized process entrypoint supplied by the recognition dispatcher.
pub trait AgentProcessMatcher: Sync {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool;
}

impl Agent {
    pub fn detects_processes(self) -> bool {
        matches!(self, Self::Codex | Self::Claude | Self::TraeCli)
    }
}

impl AgentProcessMatcher for Agent {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool {
        match self {
            Self::Codex => codex::Codex.matches_process(process),
            Self::Claude => claude::Claude.matches_process(process),
            Self::TraeCli => traecli::TraeCli.matches_process(process),
            _ => false,
        }
    }
}

pub fn recognize_process(process: &ProcessInfo<'_>) -> Option<Agent> {
    process.candidates().into_iter().find_map(|candidate| {
        KNOWN_AGENTS
            .iter()
            .copied()
            .find(|agent| agent.matches_process(&candidate))
    })
}

#[cfg(test)]
mod tests;
