use super::{AgentProcessMatcher, ProcessInfo};

pub(super) struct Pi;

impl AgentProcessMatcher for Pi {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool {
        process.matches_command(crate::PI.commands)
            || process.script.is_some_and(|path| {
                [
                    "/@earendil-works/pi-coding-agent/",
                    "/@mariozechner/pi-coding-agent/",
                ]
                .iter()
                .any(|package| path.contains(package))
                    && ["/dist/cli.js", "/dist/bundle/cli.js"]
                        .iter()
                        .any(|entry| path.ends_with(entry))
            })
    }
}
