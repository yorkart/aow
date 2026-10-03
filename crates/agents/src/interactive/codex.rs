use super::{AgentInteractive, footer};

pub(super) struct Codex;

impl AgentInteractive for Codex {
    fn input_ready(&self, lines: &[String]) -> bool {
        footer::lines(lines).any(|line| line.matches('·').count() >= 2 && model_and_directory(line))
            || compact_footer_ready(lines)
    }
}

fn model_and_directory(line: &str) -> bool {
    footer::model_loaded(line)
        && line.split('·').nth(1).is_some_and(|directory| {
            let directory = directory.trim();
            directory.starts_with('/') || directory.starts_with("~/")
        })
}

fn compact_footer_ready(lines: &[String]) -> bool {
    // Codex can put the model and directory on one line, with the idle
    // shortcuts on the next. Neither line has the two separators used by
    // the shared footer format, so recognize the pair together.
    let mut footer = footer::lines(lines);
    let Some(shortcuts) = footer.next() else {
        return false;
    };
    if !shortcuts
        .split('·')
        .any(|hint| hint.trim() == "? for shortcuts")
    {
        return false;
    }
    let Some(status) = footer.next() else {
        return false;
    };
    model_and_directory(status)
}
