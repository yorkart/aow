mod api;
mod creation;
mod store;
#[cfg(test)]
mod tests;

use crate::{AppState, HttpError};
use aow_protocol::{InstanceInfo, InstanceRef, TerminalPane, TerminalTab};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) use api::{get, list};
pub(crate) use creation::creation_parent;
pub(crate) use store::InstanceStore;

pub(crate) fn terminal(tab: &TerminalTab, pane: &TerminalPane) -> InstanceInfo {
    InstanceInfo {
        instance: InstanceRef::terminal(&pane.id),
        parent: pane.parent.clone(),
        title: tab.name.clone(),
        cwd: pane.cwd.clone(),
        agent_id: pane.agent_id.clone(),
        tab_id: Some(tab.id.clone()),
        url: Some(format!("/aow/tabs/terminal/{}", tab.id)),
        deleted: false,
    }
}

pub(crate) fn acp(session: &aow_acp::SessionInfo) -> InstanceInfo {
    InstanceInfo {
        instance: InstanceRef::acp(&session.thread_id),
        parent: session.parent.clone(),
        title: session.title.clone(),
        cwd: session.cwd.clone(),
        agent_id: Some(session.agent_id.clone()),
        tab_id: None,
        url: Some(format!("/aow/tabs/acp/{}", session.thread_id)),
        deleted: false,
    }
}

pub(crate) fn graph(state: &AppState) -> Result<BTreeMap<InstanceRef, InstanceInfo>, HttpError> {
    let mut graph = state.instances.snapshot()?;
    for item in graph.values_mut() {
        item.deleted = true;
        item.url = None;
    }
    for tab in state
        .terminals
        .list_snapshot(None)
        .map_err(crate::terminal::terminal_http_error)?
        .tabs
    {
        for pane in &tab.panes {
            let info = terminal(&tab, pane);
            graph.insert(info.instance.clone(), info);
        }
    }
    for session in state.acp.sessions(None) {
        let info = acp(&session);
        graph.insert(info.instance.clone(), info);
    }
    Ok(graph)
}

/// Validate against live instances, then preserve the entire known ancestry before creation.
/// Deletion after this point does not invalidate the historical creation relationship.
pub(crate) fn preserve_parent(
    state: &AppState,
    parent: Option<&InstanceRef>,
) -> Result<(), HttpError> {
    let Some(parent) = parent else {
        return Ok(());
    };
    parent.validate().map_err(invalid)?;
    let graph = graph(state)?;
    if !graph.get(parent).is_some_and(|info| !info.deleted) {
        return Err(missing(parent));
    }
    let mut ancestry = Vec::new();
    let mut current = Some(parent);
    let mut visited = BTreeSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            return Err(invalid("instance ancestry contains a cycle"));
        }
        let info = graph.get(id).ok_or_else(|| missing(id))?;
        ancestry.push(info.clone());
        current = info.parent.as_ref();
    }
    state.instances.record_all(ancestry)
}

pub(crate) fn invalid(message: impl Into<String>) -> HttpError {
    HttpError::new(
        axum::http::StatusCode::BAD_REQUEST,
        "invalid_instance",
        message,
        None,
    )
}

fn missing(instance: &InstanceRef) -> HttpError {
    HttpError::new(
        axum::http::StatusCode::NOT_FOUND,
        "instance_not_found",
        format!("instance {instance} not found"),
        None,
    )
}
