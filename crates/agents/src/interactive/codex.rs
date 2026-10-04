use super::AgentInteractive;

pub(super) struct Codex;

impl AgentInteractive for Codex {
    fn input_ready(&self, lines: &[String]) -> bool {
        // Ignore blank padding and recognize the directory in either footer
        // row, independently of model names, shortcut hints, or warnings.
        lines
            .iter()
            .rev()
            .filter(|line| !line.trim().is_empty())
            .take(2)
            .any(|line| {
                line.split('·').skip(1).any(|segment| {
                    let directory = segment.trim();
                    directory.starts_with('/') || directory.starts_with("~/")
                })
            })
    }
}
