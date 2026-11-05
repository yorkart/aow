use crate::{instance::*, terminal::*};
use aow_runtime_contract::instance::InstanceRef;

#[test]
fn instance_creation_rejects_unknown_fields_instead_of_losing_parentage() {
    let terminal = serde_json::json!({
        "agent":"codex", "project_id":"project", "cwd":"/repo", "timeout_seconds":120,
        "parent":{"kind":"acp", "id":"root"}
    });
    let acp = serde_json::json!({
        "agent_id":"codex-acp", "project_id":"project", "cwd":"/repo", "timeout_seconds":120,
        "parent":{"kind":"terminal", "id":"root"}
    });
    assert_eq!(
        serde_json::from_value::<AgentTerminalCreate>(terminal.clone())
            .unwrap()
            .parent,
        Some(InstanceRef::acp("root"))
    );
    assert_eq!(
        serde_json::from_value::<AcpCreate>(acp.clone())
            .unwrap()
            .parent,
        Some(InstanceRef::terminal("root"))
    );
    for field in ["parent_pane_id", "unexpected"] {
        let mut terminal = terminal.clone();
        let mut acp = acp.clone();
        terminal[field] = "removed".into();
        acp[field] = "removed".into();
        assert!(
            serde_json::from_value::<AgentTerminalCreate>(terminal)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );
        assert!(
            serde_json::from_value::<AcpCreate>(acp)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );
    }
}

#[test]
fn terminal_parent_round_trip_uses_only_instance_references() {
    for parent in [
        None,
        Some(InstanceRef::terminal("terminal-parent")),
        Some(InstanceRef::acp("acp-parent")),
    ] {
        let pane: TerminalPane = serde_json::from_value(serde_json::json!({
            "id":"pane", "parent":parent, "parent_pane_id":"removed",
            "cwd":"/repo", "shell":"/bin/sh", "status":"running", "rows":24, "cols":80,
            "created_at":"2026-01-01T00:00:00Z", "updated_at":"2026-01-01T00:00:00Z"
        }))
        .unwrap();
        assert_eq!(pane.parent, parent);
        let serialized = serde_json::to_value(&pane).unwrap();
        assert!(serialized.get("parent_pane_id").is_none());
        assert_eq!(
            serde_json::from_value::<TerminalPane>(serialized).unwrap(),
            pane
        );
    }
}

#[test]
fn terminal_layout_uses_the_shared_axis_schema() {
    let layout = TerminalLayout::Split {
        axis: TerminalSplitAxis::Row,
        ratio: 0.4,
        first: Box::new(TerminalLayout::Pane {
            pane_id: "left".to_owned(),
        }),
        second: Box::new(TerminalLayout::Pane {
            pane_id: "right".to_owned(),
        }),
    };
    let value = serde_json::to_value(layout).unwrap();
    assert_eq!(value["type"], "split");
    assert_eq!(value["axis"], "row");
    assert_eq!(value["first"]["pane_id"], "left");
    assert!(value.get("direction").is_none());
}

#[test]
fn terminal_pane_name_defaults_for_legacy_state() {
    let pane: TerminalPane = serde_json::from_value(serde_json::json!({
        "id": "pane-1",
        "cwd": "/workspace/project",
        "shell": "/bin/bash",
        "status": "running",
        "rows": 24,
        "cols": 80,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    }))
    .unwrap();

    assert_eq!(pane.name, "");
    assert_eq!(pane.kind, TerminalPaneKind::Terminal);
    assert!(pane.arguments.is_empty());
    assert!(pane.agent_id.is_none());
    assert!(pane.restart_on_daemon_restart);
    let value = serde_json::to_value(pane).unwrap();
    assert!(value.get("arguments").is_none());
    assert!(value.get("kind").is_none());
    assert!(value.get("agent_id").is_none());
    assert!(value.get("restart_on_daemon_restart").is_none());
}
