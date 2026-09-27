use super::{AgentProcessMatcher, ProcessInfo};

pub(super) struct Hermes;

impl AgentProcessMatcher for Hermes {
    fn matches_process(&self, process: &ProcessInfo<'_>) -> bool {
        process.matches_command(crate::HERMES.commands)
            || process.python_command.is_some_and(is_managed_relaunch)
    }
}

// Hermes' venv_sync.relaunch_command re-execs into its managed Python with
// an inline bootstrap. Match that complete launcher shape, never an arbitrary
// occurrence of "hermes" in Python code, a prompt or a script's arguments.
fn is_managed_relaunch(command: &str) -> bool {
    fn parse(command: &str) -> Option<()> {
        let rest = command.strip_prefix("import sys, runpy; sys.path.insert(0, ")?;
        let (_, rest) = literal(rest)?;
        let rest = rest.strip_prefix("); sys.argv = [")?;
        let (entrypoint, mut rest) = literal(rest)?;
        if !crate::HERMES
            .commands
            .contains(&entrypoint.rsplit('/').next()?)
        {
            return None;
        }
        while let Some(next) = rest.strip_prefix(", ") {
            (_, rest) = literal(next)?;
        }
        let rest = rest.strip_prefix("]; runpy.run_path(")?;
        let (script, rest) = literal(rest)?;
        (script == entrypoint && rest == ", run_name='__main__')").then_some(())
    }
    parse(command).is_some()
}

// Read, without executing, one string emitted by Python repr(). Keep escapes
// intact when comparing paths, and skip escaped quotes inside argument values.
fn literal(input: &str) -> Option<(&str, &str)> {
    let quote = *input.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut escaped = false;
    for (index, byte) in input.bytes().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == quote {
            return Some((&input[1..index], &input[index + 1..]));
        } else if matches!(byte, b'\n' | b'\r') {
            return None;
        }
    }
    None
}
