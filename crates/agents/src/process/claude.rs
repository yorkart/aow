use super::{AgentProcessMatcher, ProcessInfo};
use crate::CLAUDE;

pub(super) struct Claude;

impl AgentProcessMatcher for Claude {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool {
        process.matches_command(CLAUDE.commands)
            || process.matches_command(&["claude-code"])
            || process
                .entrypoints()
                .any(|path| path.contains("/.local/share/claude/versions/"))
            || process.script.is_some_and(|path| {
                path.contains("/@anthropic-ai/claude-code/") && path.ends_with("/cli.js")
            })
    }
}
