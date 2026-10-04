use super::*;

pub(in crate::runtime) struct ActiveStream {
    // Only the controller has an owner. Read-only observers receive the same
    // stream with `None`, which deliberately prevents input and resize.
    pub(super) owner: Option<ControllerOwner>,
    pub(in crate::runtime) expected_offset: u64,
    pub(super) output_closed: bool,
    pub(super) pending_status: Option<(TerminalPaneStatus, Option<u32>)>,
    pub(in crate::runtime) events: broadcast::Receiver<RuntimeEvent>,
    pub(super) controller_changed: watch::Receiver<Option<ControllerOwner>>,
}

pub(in crate::runtime) enum BeginClaim {
    Claimed(ClaimedAttachment),
    Waiting,
    Unchanged,
    End,
}

pub(in crate::runtime) async fn begin_claim<S>(
    sender: &mut S,
    connection: &RuntimeConnection,
    force: bool,
    announce: bool,
    current_owner: Option<ControllerOwner>,
    suppress_waiting_announcement: bool,
) -> BeginClaim
where
    S: Sink<Message> + Unpin,
{
    let runtime = &connection.runtime;
    let gate = runtime.delivery_gate.lock().await;
    let outcome = match runtime.claim(
        connection.attachment_id,
        connection.resume_after,
        force,
        connection.vt_snapshot,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            drop(gate);
            let _ = send_socket_error(sender, "claim_failed", &error.to_string()).await;
            return BeginClaim::End;
        }
    };
    match outcome {
        ClaimOutcome::Unchanged => BeginClaim::Unchanged,
        ClaimOutcome::Waiting => {
            // A slow observer must never retain the ownership-delivery gate.
            // It is safe to announce Waiting after the claim decision because
            // the message carries no generation or stream boundary.
            drop(gate);
            if suppress_waiting_announcement {
                return BeginClaim::Waiting;
            }
            if send_control(sender, TerminalControlState::Waiting)
                .await
                .is_err()
            {
                BeginClaim::End
            } else {
                BeginClaim::Waiting
            }
        }
        ClaimOutcome::Claimed(claimed) => {
            // Keep the successful claim and its ACK in the same gated
            // critical section. Every send is bounded, so a client that has
            // stopped reading can delay a takeover by at most one frame.
            if announce
                && send_control(sender, TerminalControlState::Claimed)
                    .await
                    .is_err()
            {
                runtime.release(claimed.owner);
                return BeginClaim::End;
            }
            // Normally an attachment that already owns the controller returns
            // Unchanged above. Keep this conditional cleanup for a caller that
            // entered with a stale locally-held identity.
            if let Some(old_owner) = current_owner
                && old_owner != claimed.owner
            {
                runtime.release(old_owner);
            }
            BeginClaim::Claimed(claimed)
        }
    }
}

pub(super) async fn begin_observation<S>(
    sender: &mut S,
    connection: &RuntimeConnection,
) -> Option<ActiveStream>
where
    S: Sink<Message> + Unpin,
{
    let observed = match connection
        .runtime
        .observe(connection.resume_after, connection.vt_snapshot)
    {
        Ok(observed) => observed,
        Err(error) => {
            let _ = send_socket_error(sender, "observe_failed", &error.to_string()).await;
            return None;
        }
    };
    if send_control(sender, TerminalControlState::Observing)
        .await
        .is_err()
    {
        return None;
    }
    super::initialization::initialize_observer_stream(sender, &connection.runtime, observed).await
}
