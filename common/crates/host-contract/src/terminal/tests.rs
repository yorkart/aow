use super::*;

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
            status: TerminalRuntimeStatus::Exited,
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
            restore_gzip: None,
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
            restore_gzip: None,
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
            restore_gzip: None,
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
                restore_gzip: None,
                restore_cols: Some(dimension),
                restore_rows: Some(dimension),
            }
        );
    }
}
