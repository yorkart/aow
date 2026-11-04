//! Footer primitives; layout recognition belongs to each Agent adapter.

pub(super) fn lines(lines: &[String]) -> impl Iterator<Item = &str> {
    // Ignore blank padding below the TUI, then inspect only the footer.
    lines
        .iter()
        .rev()
        .skip_while(|line| line.trim().is_empty())
        .take(3)
        .map(String::as_str)
}

pub(super) fn model_loaded(line: &str) -> bool {
    line.split('·').next().is_some_and(|model| {
        !model.trim().is_empty() && !model.to_ascii_lowercase().contains("loading")
    })
}
