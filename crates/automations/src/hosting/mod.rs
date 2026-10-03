//! The small output convention used only by hosted manual runs.

pub const FINISH_MARKER: &str = "[AOW_HOSTING_DONE]";

pub(crate) fn prompt(task: &str) -> String {
    format!(
        "你正在执行一次审查任务，请遵循任务原有的执行和输出要求。审查通过时，在最终输出的最后另起一行，单独追加标记：{FINISH_MARKER}。\n\n审查任务：\n{task}\n"
    )
}

/// Only a standalone final marker counts. Quoted examples, substrings and
/// fenced code (including unfinished fences) never terminate hosting.
pub fn review_passed(output: &str) -> bool {
    let mut fence: Option<(char, usize)> = None;
    let mut passed = false;
    for raw in output.lines().filter(|line| !line.trim().is_empty()) {
        let line = raw.trim();
        passed = line == FINISH_MARKER
            && fence.is_none()
            && !raw.starts_with("    ")
            && !raw.starts_with('\t');
        let first = line.chars().next().unwrap();
        if !matches!(first, '`' | '~') {
            continue;
        }
        let length = line.chars().take_while(|ch| *ch == first).count();
        match fence {
            Some((marker, opening))
                if first == marker && length >= opening && line[length..].trim().is_empty() =>
            {
                fence = None
            }
            None if length >= 3 => fence = Some((first, length)),
            _ => {}
        }
    }
    passed
}

#[cfg(test)]
mod tests;
