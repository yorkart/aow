use super::super::*;
use aow_protocol::AgentTerminalInfo;

pub(super) fn info(tab: &TerminalTab, pane: &TerminalPane) -> Option<AgentTerminalInfo> {
    Some(AgentTerminalInfo {
        pane_id: pane.id.clone(),
        parent_pane_id: pane.parent_pane_id.clone(),
        tab_id: tab.id.clone(),
        cwd: pane.cwd.clone(),
        agent: pane.agent_id.clone()?,
        status: pane.status,
        state: pane.agent_terminal.clone()?,
    })
}
