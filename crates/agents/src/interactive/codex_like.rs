//! Shared screen format used only by the Codex and TraeCode CLI adapters.

pub(super) fn input_ready(lines: &[String]) -> bool {
    // Ignore blank padding below the TUI, then inspect only the footer.
    // Do not count dots across lines or across repeated redraws.
    lines
        .iter()
        .rev()
        .skip_while(|line| line.trim().is_empty())
        .take(3)
        .any(|line| {
            line.matches('·').count() >= 2
                && line.split('·').next().is_some_and(|model| {
                    !model.trim().is_empty() && !model.to_ascii_lowercase().contains("loading")
                })
        })
}
