use super::*;

pub(super) fn validate_ratio(ratio: f32) -> Result<f32, TerminalError> {
    if !ratio.is_finite() || !(0.05..=0.95).contains(&ratio) {
        return Err(TerminalError::Invalid(
            "split ratio must be between 0.05 and 0.95".to_owned(),
        ));
    }
    Ok(ratio)
}

pub(super) fn tab_index(state: &ManagerState, tab_id: &str) -> Result<usize, TerminalError> {
    state
        .tabs
        .iter()
        .position(|tab| tab.id == tab_id)
        .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))
}

pub(super) fn touch_tab(tab: &mut TerminalTab) {
    tab.revision = tab.revision.saturating_add(1);
    tab.updated_at = timestamp();
}

pub(super) fn replace_layout_leaf(
    layout: &mut TerminalLayout,
    pane_id: &str,
    replacement: &TerminalLayout,
) -> bool {
    match layout {
        TerminalLayout::Pane { pane_id: current } if current == pane_id => {
            *layout = replacement.clone();
            true
        }
        TerminalLayout::Pane { .. } => false,
        TerminalLayout::Split { first, second, .. } => {
            replace_layout_leaf(first, pane_id, replacement)
                || replace_layout_leaf(second, pane_id, replacement)
        }
    }
}

pub(super) fn remove_layout_leaf(layout: TerminalLayout, pane_id: &str) -> Option<TerminalLayout> {
    match layout {
        TerminalLayout::Pane { pane_id: current } => {
            (current != pane_id).then_some(TerminalLayout::Pane { pane_id: current })
        }
        TerminalLayout::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            if layout_contains(&first, pane_id) {
                match remove_layout_leaf(*first, pane_id) {
                    Some(first) => Some(TerminalLayout::Split {
                        axis,
                        ratio,
                        first: Box::new(first),
                        second,
                    }),
                    None => Some(*second),
                }
            } else {
                match remove_layout_leaf(*second, pane_id) {
                    Some(second) => Some(TerminalLayout::Split {
                        axis,
                        ratio,
                        first,
                        second: Box::new(second),
                    }),
                    None => Some(*first),
                }
            }
        }
    }
}

pub(super) fn layout_contains(layout: &TerminalLayout, pane_id: &str) -> bool {
    match layout {
        TerminalLayout::Pane { pane_id: current } => current == pane_id,
        TerminalLayout::Split { first, second, .. } => {
            layout_contains(first, pane_id) || layout_contains(second, pane_id)
        }
    }
}

pub(super) fn validate_layout(
    layout: &TerminalLayout,
    panes: &[TerminalPane],
) -> Result<(), TerminalError> {
    let expected = panes
        .iter()
        .map(|pane| pane.id.as_str())
        .collect::<HashSet<_>>();
    let mut found = HashSet::new();
    collect_layout_panes(layout, 0, &mut found)?;
    if found != expected {
        return Err(TerminalError::Invalid(
            "layout must contain every pane exactly once".to_owned(),
        ));
    }
    Ok(())
}

fn collect_layout_panes<'a>(
    layout: &'a TerminalLayout,
    depth: usize,
    panes: &mut HashSet<&'a str>,
) -> Result<(), TerminalError> {
    if depth > MAX_LAYOUT_DEPTH {
        return Err(TerminalError::Invalid(format!(
            "layout exceeds maximum depth {MAX_LAYOUT_DEPTH}"
        )));
    }
    match layout {
        TerminalLayout::Pane { pane_id } => {
            if !panes.insert(pane_id) {
                return Err(TerminalError::Invalid(format!(
                    "pane appears more than once in layout: {pane_id}"
                )));
            }
        }
        TerminalLayout::Split {
            ratio,
            first,
            second,
            ..
        } => {
            validate_ratio(*ratio)?;
            collect_layout_panes(first, depth + 1, panes)?;
            collect_layout_panes(second, depth + 1, panes)?;
        }
    }
    Ok(())
}

pub(super) fn validate_persisted_tabs(tabs: &[TerminalTab]) -> Result<(), TerminalError> {
    let mut tab_ids = HashSet::new();
    let mut pane_ids = HashSet::new();
    for tab in tabs {
        if !tab_ids.insert(tab.id.as_str()) {
            return Err(TerminalError::Invalid(format!(
                "duplicate terminal tab id: {}",
                tab.id
            )));
        }
        if tab.panes.is_empty() {
            return Err(TerminalError::Invalid(format!(
                "terminal tab has no panes: {}",
                tab.id
            )));
        }
        for pane in &tab.panes {
            if !pane_ids.insert(pane.id.as_str()) {
                return Err(TerminalError::Invalid(format!(
                    "duplicate terminal pane id: {}",
                    pane.id
                )));
            }
            validate_name(&pane.name)?;
        }
        validate_layout(&tab.layout, &tab.panes)?;
    }
    Ok(())
}
