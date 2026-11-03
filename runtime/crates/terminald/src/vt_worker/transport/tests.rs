use std::sync::atomic::Ordering;

use serde_json::json;

use super::{client::mutation_response, protocol::Response};
use crate::vt_worker::{session::VtSession, tests::test_client};

fn acknowledgement(session: &VtSession, applied: u64, bytes: u64) -> Response {
    Response {
        metadata: json!({
            "id": 1, "action": "write", "ok": true,
            "result": { "session_id": session.state.session_id,
                "generation": session.state.generation,
                "applied_offset": applied, "byte_length": bytes },
        }),
        data: Vec::new(),
    }
}

#[test]
fn write_acks_advance_only_the_contiguous_sent_prefix() {
    for (applied, bytes) in [(3, 3), (5, 1), (9, 6)] {
        let (client, _receiver) = test_client(2);
        let session = client.create_session("epoch".into(), 80, 24);
        session.state.sent_offset.store(8, Ordering::Release);
        mutation_response(acknowledgement(&session, 3, 3), &client.sessions).unwrap();
        assert_eq!(*session.applied_offsets().borrow(), Some(3));
        assert!(
            mutation_response(acknowledgement(&session, applied, bytes), &client.sessions).is_err()
        );
        assert_eq!(*session.applied_offsets().borrow(), None);
        assert!(
            !session.state.is_active(),
            "duplicate, gap and future ACKs stop publication"
        );
    }
}

#[test]
fn write_acks_do_not_cross_session_generations() {
    let (client, _receiver) = test_client(2);
    let session = client.create_session("epoch".into(), 80, 24);
    session.state.sent_offset.store(8, Ordering::Release);
    let mut stale = acknowledgement(&session, 3, 3);
    stale.metadata["result"]["generation"] = json!("old-epoch");
    mutation_response(stale, &client.sessions).unwrap();
    assert_eq!(*session.applied_offsets().borrow(), Some(0));
    mutation_response(acknowledgement(&session, 3, 3), &client.sessions).unwrap();
    mutation_response(acknowledgement(&session, 8, 5), &client.sessions).unwrap();
    assert_eq!(*session.applied_offsets().borrow(), Some(8));
}

#[test]
fn malformed_ack_does_not_publish_output() {
    let (client, _receiver) = test_client(2);
    let session = client.create_session("epoch".into(), 80, 24);
    session.state.sent_offset.store(8, Ordering::Release);
    let mut ack = acknowledgement(&session, 3, 3);
    ack.data.push(1);
    assert!(mutation_response(ack, &client.sessions).is_err());
    let mut ack = acknowledgement(&session, 3, 3);
    ack.metadata["result"]["applied_offset"] = json!("3");
    assert!(mutation_response(ack, &client.sessions).is_err());
    assert_eq!(*session.applied_offsets().borrow(), Some(0));
}
