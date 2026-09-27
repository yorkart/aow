use super::{AgentProcessMatcher, ProcessInfo};
use crate::CODEX;

pub(super) struct Codex;

impl AgentProcessMatcher for Codex {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool {
        process.matches_command(CODEX.commands)
            || process.script.is_some_and(|path| {
                path.contains("/@openai/codex/") && path.ends_with("/bin/codex.js")
            })
    }
}
