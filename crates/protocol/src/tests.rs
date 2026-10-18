use super::*;

#[test]
fn file_kind_uses_snake_case() {
    assert_eq!(
        serde_json::to_string(&FileKind::Directory).unwrap(),
        "\"directory\""
    );
}

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

#[test]
fn terminal_attach_messages_match_the_browser_wire_format() {
    assert_eq!(
        serde_json::to_value(TerminalAttachClientMessage::Claim { force: true }).unwrap(),
        serde_json::json!({"type": "claim", "force": true})
    );
    assert_eq!(
        serde_json::to_value(TerminalAttachClientMessage::Claim { force: false }).unwrap(),
        serde_json::json!({"type": "claim", "force": false})
    );
    assert_eq!(
        serde_json::to_value(TerminalAttachClientMessage::Resize {
            cols: 120,
            rows: 40
        })
        .unwrap(),
        serde_json::json!({"type": "resize", "cols": 120, "rows": 40})
    );
    assert_eq!(
        serde_json::to_value(TerminalAttachServerMessage::Control {
            state: TerminalControlState::Waiting,
        })
        .unwrap(),
        serde_json::json!({"type": "control", "state": "waiting"})
    );
    assert_eq!(
        serde_json::to_value(TerminalAttachServerMessage::Control {
            state: TerminalControlState::Claimed,
        })
        .unwrap(),
        serde_json::json!({"type": "control", "state": "claimed"})
    );
    assert_eq!(
        serde_json::to_value(TerminalAttachServerMessage::Control {
            state: TerminalControlState::Observing,
        })
        .unwrap(),
        serde_json::json!({"type": "control", "state": "observing"})
    );
    assert_eq!(
        serde_json::to_value(TerminalAttachServerMessage::Status {
            status: TerminalPaneStatus::Exited,
            exit_code: Some(7),
        })
        .unwrap(),
        serde_json::json!({"type": "status", "status": "exited", "exit_code": 7})
    );
    assert_eq!(
        serde_json::to_value(TerminalAttachServerMessage::Stream {
            epoch: "runtime-epoch".to_owned(),
            offset: 42,
            reset: true,
            replay_bytes: 1024,
            restore: Some("\u{1b}[?2004h".to_owned()),
            restore_cols: Some(120),
            restore_rows: Some(40),
        })
        .unwrap(),
        serde_json::json!({
            "type": "stream",
            "epoch": "runtime-epoch",
            "offset": 42,
            "reset": true,
            "replay_bytes": 1024,
            "restore": "\u{1b}[?2004h",
            "restore_cols": 120,
            "restore_rows": 40
        })
    );
    let legacy_stream: TerminalAttachServerMessage = serde_json::from_value(serde_json::json!({
        "type": "stream",
        "epoch": "runtime-epoch",
        "offset": 42,
        "reset": false,
        "replay_bytes": 0
    }))
    .unwrap();
    assert_eq!(
        legacy_stream,
        TerminalAttachServerMessage::Stream {
            epoch: "runtime-epoch".to_owned(),
            offset: 42,
            reset: false,
            replay_bytes: 0,
            restore: None,
            restore_cols: None,
            restore_rows: None,
        }
    );
    let legacy_stream_json = serde_json::to_value(legacy_stream).unwrap();
    assert!(legacy_stream_json.get("restore").is_none());
    assert!(legacy_stream_json.get("restore_cols").is_none());
    assert!(legacy_stream_json.get("restore_rows").is_none());
    assert_eq!(
        serde_json::to_value(TerminalAttachServerMessage::Resized {
            cols: 120,
            rows: 40,
        })
        .unwrap(),
        serde_json::json!({"type": "resized", "cols": 120, "rows": 40})
    );
}

#[test]
fn terminal_stream_restore_metadata_remains_backward_compatible() {
    let legacy_mode_restore: TerminalAttachServerMessage =
        serde_json::from_value(serde_json::json!({
            "type": "stream",
            "epoch": "runtime-epoch",
            "offset": 42,
            "reset": true,
            "replay_bytes": 0,
            "restore": "\u{001b}[?2004h"
        }))
        .unwrap();
    assert_eq!(
        legacy_mode_restore,
        TerminalAttachServerMessage::Stream {
            epoch: "runtime-epoch".to_owned(),
            offset: 42,
            reset: true,
            replay_bytes: 0,
            restore: Some("\u{1b}[?2004h".to_owned()),
            restore_cols: None,
            restore_rows: None,
        }
    );

    for dimension in [1_u16, 1000_u16] {
        let snapshot: TerminalAttachServerMessage = serde_json::from_value(serde_json::json!({
            "type": "stream",
            "epoch": "runtime-epoch",
            "offset": 42,
            "reset": true,
            "replay_bytes": 0,
            "restore": "snapshot",
            "restore_cols": dimension,
            "restore_rows": dimension
        }))
        .unwrap();
        assert_eq!(
            snapshot,
            TerminalAttachServerMessage::Stream {
                epoch: "runtime-epoch".to_owned(),
                offset: 42,
                reset: true,
                replay_bytes: 0,
                restore: Some("snapshot".to_owned()),
                restore_cols: Some(dimension),
                restore_rows: Some(dimension),
            }
        );
    }
}
