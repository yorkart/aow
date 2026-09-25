use super::AgentInteractive;

pub(super) struct Hermes;

impl AgentInteractive for Hermes {
    fn input_ready(&self, lines: &[String]) -> bool {
        // Classic Hermes renders a bare prompt (or "profile ❯") while idle.
        // Approval, clarification and working states prepend another symbol
        // or put placeholder text after it; neither may receive a task.
        lines.iter().rev().take(8).any(|line| {
            let text = line.trim().trim_matches('│').trim();
            if text == "❯" {
                return true;
            }
            text.strip_suffix(" ❯").is_some_and(|profile| {
                !profile.is_empty()
                    && profile
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || "_-".contains(ch))
            })
        })
    }
}
