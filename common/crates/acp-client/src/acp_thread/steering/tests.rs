use super::*;
use crate::acp_thread::{AcpThread, turns};
use serde_json::json;

#[test]
fn a_host_restart_marks_unacknowledged_delivery_unknown() {
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "generic".into(),
        "/repo".into(),
    );
    thread.steering_pending = true;
    AcpThread::push(
        &mut thread,
        "steering",
        json!({"type":"text", "text":"Keep going"}),
    );
    thread.entries[0].delivery = Some(InputDelivery {
        status: "sending".into(),
        error: None,
    });
    let mut restored: SessionSnapshot =
        serde_json::from_value(serde_json::to_value(&thread).unwrap()).unwrap();
    disconnected(&mut restored);
    assert!(!restored.steering_pending);
    assert_eq!(
        restored.entries[0].delivery.as_ref().unwrap().status,
        "unknown"
    );
}

#[test]
fn replay_preserves_verified_steering_entries_and_the_original_turn() {
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "generic".into(),
        "/repo".into(),
    );
    for (kind, text) in [
        ("user", "Task"),
        ("assistant", "Working"),
        ("steering", "Correction"),
        ("assistant", "Done"),
    ] {
        AcpThread::push(&mut thread, kind, json!({"type":"text", "text":text}));
    }
    turns::start(&mut thread, 0);
    thread.stop_reason = Some("end_turn".into());
    turns::finish(&mut thread);
    thread.entries[2].delivery = Some(InputDelivery {
        status: "injected".into(),
        error: None,
    });
    let mut replay = thread.clone();
    replay.turns.clear();
    replay.entries[2].kind = "user".into();
    replay.entries[2].delivery = None;
    turns::restore(&thread, &mut replay);
    assert_eq!(replay.turns.len(), 1);
    assert_eq!(replay.entries[2].kind, "steering");
    assert_eq!(
        crate::acp_thread::overview::snapshot(&mut replay)
            .overview
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn detach_tracking_ignores_old_idle_and_never_guesses_success() {
    let mut thread = AcpThread::new_snapshot(
        "local".into(),
        "remote".into(),
        "generic".into(),
        "/repo".into(),
    );
    AcpThread::push(&mut thread, "user", json!({"type":"text", "text":"Task"}));
    turns::start(&mut thread, 0);
    thread.status = "working".into();
    thread.steering.detached = true;
    let status = |value| json!({"sessionUpdate":"session_info_update", "_meta":{"codex":{"threadStatus":{"type":value}}}});
    observe_activity(&mut thread, &status("idle"));
    assert_eq!(thread.status, "working");
    observe_activity(&mut thread, &status("active"));
    observe_activity(&mut thread, &status("idle"));
    assert_eq!(thread.status, "idle");
    assert_eq!(thread.turns[0].status, "unknown");
    assert!(thread.stop_reason.is_none());
}
