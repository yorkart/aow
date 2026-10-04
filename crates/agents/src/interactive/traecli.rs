use super::{AgentInteractive, footer};

pub(super) struct TraeCli;

impl AgentInteractive for TraeCli {
    fn input_ready(&self, lines: &[String]) -> bool {
        // Trae CLI can show context usage between the model and directory.
        footer::lines(lines)
            .any(|line| line.matches('·').count() >= 2 && footer::model_loaded(line))
    }
}
