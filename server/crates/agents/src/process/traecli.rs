use super::{AgentProcessMatcher, ProcessInfo};
use crate::TRAECLI;

pub(super) struct TraeCli;

impl AgentProcessMatcher for TraeCli {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool {
        // argv[0] retains the public launch entrypoint when `traecli` is a
        // symlink. Its target name and installation layout are implementation
        // details and can change independently of the command.
        process.matches_command(TRAECLI.commands)
    }
}
