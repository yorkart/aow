use super::AgentInteractive;

pub(super) struct Pi;

impl AgentInteractive for Pi {
    fn input_ready(&self, lines: &[String]) -> bool {
        // Pi's default editor is an empty line between two plain horizontal
        // borders, followed by cwd and context/model information. A working
        // indicator replaces the top border; dialogs replace the editor.
        let lines: Vec<_> = lines.iter().map(|line| line.trim()).collect();
        lines.windows(5).rev().take(8).any(|window| {
            let border = |text: &str| text.chars().count() >= 4 && text.chars().all(|c| c == '─');
            border(window[0])
                && window[1].is_empty()
                && border(window[2])
                && (window[3].starts_with('/') || window[3].starts_with('~'))
                && (window[4].contains("%/") || window[4].contains("?/"))
                && !window[4].contains("no-model")
        })
    }
}
