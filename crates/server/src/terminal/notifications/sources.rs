use std::{collections::HashMap, path::Path};

use aow_protocol::{TerminalAgentList, TerminalPaneStatus, TerminalTab};

use super::{TaskStopNotification, TaskStopSource};

pub(super) async fn add_sources(
    events: &mut [TaskStopNotification],
    tabs: &[TerminalTab],
    detected: Option<&TerminalAgentList>,
    aow: &crate::aow::AowManager,
) {
    let mut project_names = HashMap::new();
    for event in events {
        for tab in tabs {
            if !tab
                .panes
                .iter()
                .any(|pane| event.instance_ids.contains(&pane.id))
            {
                continue;
            }
            let project_name = match project_names.get(&tab.workspace_root) {
                Some(name) => name,
                None => {
                    let name = aow
                        .project_name_for_workspace(&tab.workspace_root)
                        .await
                        .unwrap_or_else(|| {
                            Path::new(&tab.workspace_root)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or(&tab.workspace_root)
                                .to_owned()
                        });
                    project_names
                        .entry(tab.workspace_root.clone())
                        .or_insert(name)
                }
            };
            event.sources.push(TaskStopSource {
                project_name: project_name.clone(),
                workspace_root: tab.workspace_root.clone(),
                tab_id: tab.id.clone(),
                tab_name: displayed_tab_name(tab, detected),
                tab_url: None,
            });
        }
    }
}

// Keep the label consistent with frontend terminalTabPresentation: explicit
// names win, otherwise one agent supplies the title and several show a count.
fn displayed_tab_name(tab: &TerminalTab, detected: Option<&TerminalAgentList>) -> String {
    if tab.name_is_custom != Some(false) {
        return tab.name.clone();
    }
    let agents: Vec<_> = tab
        .panes
        .iter()
        .filter(|pane| {
            pane.status == TerminalPaneStatus::Running
                && detected
                    .and_then(|list| list.agents.get(&pane.id))
                    .unwrap_or(&pane.agent_id)
                    .is_some()
        })
        .collect();
    match agents.as_slice() {
        [pane] => detected
            .and_then(|list| list.titles.get(&pane.id))
            .filter(|title| !title.is_empty())
            .unwrap_or(&tab.name)
            .clone(),
        [] => tab.name.clone(),
        _ => format!("{} agents", agents.len()),
    }
}
