mod attachment;
mod mutations;

use super::*;

pub(super) fn find_pane(tabs: &[TerminalTab], pane_id: &str) -> Option<(usize, usize)> {
    for (tab_index, tab) in tabs.iter().enumerate() {
        if let Some(index) = tab.panes.iter().position(|pane| pane.id == pane_id) {
            return Some((tab_index, index));
        }
    }
    None
}
