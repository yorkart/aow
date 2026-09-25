use super::{AgentProcessMatcher, ProcessInfo};

pub(super) struct Hermes;

impl AgentProcessMatcher for Hermes {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool {
        process.matches_command(crate::HERMES.commands)
    }
}
