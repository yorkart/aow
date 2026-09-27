//! WebSocket attachment, replay, and controller hand-off.

use super::*;

pub(super) struct ActiveStream {
    // Only the controller has an owner. Read-only observers receive the same
    // stream with `None`, which deliberately prevents input and resize.
    owner: Option<ControllerOwner>,
    pub(super) expected_offset: u64,
    output_closed: bool,
    pending_status: Option<(TerminalPaneStatus, Option<u32>)>,
    pub(super) events: broadcast::Receiver<RuntimeEvent>,
    controller_changed: watch::Receiver<Option<ControllerOwner>>,
}

struct StreamAttachment {
    stream_epoch: String,
    stream_offset: u64,
    reset: bool,
    replay: Vec<u8>,
    replay_bytes: u64,
    next_offset: u64,
    owner: Option<ControllerOwner>,
    controller_changed: watch::Receiver<Option<ControllerOwner>>,
    restore: Option<String>,
    restore_cols: Option<u16>,
    restore_rows: Option<u16>,
}

impl From<ClaimedAttachment> for StreamAttachment {
    fn from(claimed: ClaimedAttachment) -> Self {
        Self {
            stream_epoch: claimed.stream_epoch,
            stream_offset: claimed.stream_offset,
            reset: claimed.reset,
            replay: claimed.replay,
            replay_bytes: claimed.replay_bytes,
            next_offset: claimed.next_offset,
            owner: Some(claimed.owner),
            controller_changed: claimed.controller_changed,
            restore: claimed.restore,
            restore_cols: claimed.restore_cols,
            restore_rows: claimed.restore_rows,
        }
    }
}

impl From<ObservedAttachment> for StreamAttachment {
    fn from(observed: ObservedAttachment) -> Self {
        Self {
            stream_epoch: observed.stream_epoch,
            stream_offset: observed.stream_offset,
            reset: observed.reset,
            replay: observed.replay,
            replay_bytes: observed.replay_bytes,
            next_offset: observed.next_offset,
            owner: None,
            controller_changed: observed.controller_changed,
            restore: observed.restore,
            restore_cols: observed.restore_cols,
            restore_rows: observed.restore_rows,
        }
    }
}

pub(super) enum BeginClaim {
    Claimed(ClaimedAttachment),
    Waiting,
    Unchanged,
    End,
}

pub(super) async fn runtime_socket(
    socket: WebSocket,
    mut connection: RuntimeConnection,
    controlled: bool,
    observe: bool,
) {
    let (mut sender, mut receiver) = socket.split();
    let runtime = connection.runtime.clone();
    let mut held_owner = None;
    let mut active = None;
    let mut waiting = false;

    if !controlled {
        match begin_claim(&mut sender, &connection, true, false, None, false).await {
            BeginClaim::Claimed(claimed) => {
                held_owner = Some(claimed.owner);
                active = initialize_stream(&mut sender, &runtime, claimed).await;
                if active.is_none() {
                    runtime.release(held_owner.expect("legacy claim assigned an owner"));
                    let _ = bounded_close(&mut sender).await;
                    return;
                }
            }
            BeginClaim::Waiting => unreachable!("forced legacy claim cannot wait"),
            BeginClaim::Unchanged => unreachable!("legacy connection has not claimed yet"),
            BeginClaim::End => {
                let _ = bounded_close(&mut sender).await;
                return;
            }
        }
    }

    loop {
        let Some(mut current) = active.take() else {
            tokio::select! {
                incoming = receiver.next() => {
                    let Some(incoming) = incoming else { break };
                    match incoming {
                        Ok(Message::Binary(_)) => {
                            if send_controller_required(&mut sender).await.is_err() { break; }
                        }
                        Ok(Message::Text(text)) => {
                            match serde_json::from_str::<TerminalAttachClientMessage>(&text) {
                                Ok(TerminalAttachClientMessage::Observe) => {
                                    waiting = false;
                                    active = begin_observation(&mut sender, &connection).await;
                                    if active.is_none() { break; }
                                }
                                Ok(TerminalAttachClientMessage::Claim { force }) => {
                                    match begin_claim(&mut sender, &connection, force, true, None, observe).await {
                                        BeginClaim::Claimed(claimed) => {
                                            waiting = false;
                                            held_owner = Some(claimed.owner);
                                            active = initialize_stream(&mut sender, &runtime, claimed).await;
                                            if active.is_none() { break; }
                                        }
                                        BeginClaim::Waiting if observe => {
                                            active = begin_observation(&mut sender, &connection).await;
                                            if active.is_none() { break; }
                                        }
                                        BeginClaim::Waiting => waiting = true,
                                        BeginClaim::Unchanged => {}
                                        BeginClaim::End => break,
                                    }
                                }
                                Ok(TerminalAttachClientMessage::Resize { .. } | TerminalAttachClientMessage::Write { .. }) => {
                                    if send_controller_required(&mut sender).await.is_err() { break; }
                                }
                                Err(error) => {
                                    if send_socket_error(&mut sender, "invalid_control", &error.to_string()).await.is_err() { break; }
                                }
                            }
                        }
                        Ok(Message::Close(_)) | Err(_) => break,
                        Ok(Message::Ping(_) | Message::Pong(_)) => {}
                    }
                }
                changed = connection.deleted_changed.changed() => {
                    if changed.is_err() || *connection.deleted_changed.borrow_and_update() {
                        let _ = send_socket_error(&mut sender, "terminal_deleted", "terminal runtime was deleted").await;
                        break;
                    }
                }
                changed = connection.controller_changed.changed(), if waiting => {
                    if changed.is_err() {
                        break;
                    }
                    connection.controller_changed.borrow_and_update();
                    // `watch` may coalesce a fast vacant -> newly-owned pair.
                    // Retrying on every revision lets exactly one waiter claim
                    // the mutex-protected vacancy while every loser receives a
                    // fresh waiting response and remains eligible for the next
                    // release.
                    match begin_claim(&mut sender, &connection, false, true, None, false).await {
                        BeginClaim::Claimed(claimed) => {
                            waiting = false;
                            held_owner = Some(claimed.owner);
                            active = initialize_stream(&mut sender, &runtime, claimed).await;
                            if active.is_none() { break; }
                        }
                        BeginClaim::Waiting => {}
                        BeginClaim::Unchanged => {}
                        BeginClaim::End => break,
                    }
                }
            }
            continue;
        };

        if let Some(owner) = current.owner {
            match runtime.is_owner(owner) {
                Ok(true) => {}
                Ok(false) => {
                    let _gate = runtime.delivery_gate.lock().await;
                    if !runtime.is_owner(owner).unwrap_or(false) {
                        let _ = send_attachment_superseded(&mut sender).await;
                        break;
                    }
                }
                Err(error) => {
                    let _ = send_socket_error(&mut sender, "controller_failed", &error.to_string())
                        .await;
                    break;
                }
            }
        }

        tokio::select! {
            incoming = receiver.next() => {
                let Some(incoming) = incoming else { break };
                match incoming {
                    Ok(Message::Binary(bytes)) => {
                        if let Some(owner) = current.owner {
                            let _gate = runtime.delivery_gate.lock().await;
                            let write_runtime = runtime.clone();
                            let result = tokio::task::spawn_blocking(move || write_runtime.write_input(owner, &bytes)).await;
                            match result {
                                Ok(Ok(true)) => active = Some(current),
                                Ok(Ok(false)) => {
                                    let _ = send_attachment_superseded(&mut sender).await;
                                    break;
                                }
                                Ok(Err(error)) => {
                                    if send_socket_error(&mut sender, "input_failed", &error.to_string()).await.is_err() { break; }
                                    active = Some(current);
                                }
                                Err(error) => {
                                    if send_socket_error(&mut sender, "input_failed", &error.to_string()).await.is_err() { break; }
                                    active = Some(current);
                                }
                            }
                        } else if send_controller_required(&mut sender).await.is_err() {
                            break;
                        } else {
                            active = Some(current);
                        }
                    }
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<TerminalAttachClientMessage>(&text) {
                            Ok(TerminalAttachClientMessage::Observe) => {
                                if let Some(owner) = held_owner.take() { runtime.release(owner); }
                                active = begin_observation(&mut sender, &connection).await;
                                if active.is_none() { break; }
                            }
                            Ok(TerminalAttachClientMessage::Write { request_id, data }) => {
                                if request_id.len() > 128 || data.len() > 256 * 1024 {
                                    if send_socket_error(&mut sender, "invalid_input", "input exceeds size limit").await.is_err() { break; }
                                } else if let Some(owner) = current.owner {
                                    let _gate = runtime.delivery_gate.lock().await;
                                    let write_runtime = runtime.clone();
                                    let result = tokio::task::spawn_blocking(move || write_runtime.write_input(owner, data.as_bytes())).await;
                                    match result {
                                        Ok(Ok(true)) => {
                                            let text = serde_json::to_string(&TerminalAttachServerMessage::Written { request_id }).expect("write acknowledgement");
                                            if bounded_send(&mut sender, Message::Text(text.into())).await.is_err() { break; }
                                        }
                                        Ok(Ok(false)) => { let _ = send_attachment_superseded(&mut sender).await; break; }
                                        _ => { let _ = send_socket_error(&mut sender, "input_failed", "PTY input failed").await; break; }
                                    }
                                } else if send_controller_required(&mut sender).await.is_err() { break; }
                                active = Some(current);
                            }
                            Ok(TerminalAttachClientMessage::Claim { force }) => {
                                match begin_claim(&mut sender, &connection, force, controlled, current.owner, false).await {
                                    BeginClaim::Claimed(claimed) => {
                                        held_owner = Some(claimed.owner);
                                        active = initialize_stream(&mut sender, &runtime, claimed).await;
                                        if active.is_none() { break; }
                                    }
                                    BeginClaim::Waiting => {
                                        active = Some(current);
                                    }
                                    BeginClaim::Unchanged => active = Some(current),
                                    BeginClaim::End => break,
                                }
                            }
                            Ok(TerminalAttachClientMessage::Resize { cols, rows }) => {
                                if let Some(owner) = current.owner {
                                    let _gate = runtime.delivery_gate.lock().await;
                                    match runtime.resize(owner, rows, cols) {
                                        Ok(true) => {
                                            if send_resized(&mut sender, cols, rows).await.is_err() { break; }
                                            active = Some(current);
                                        }
                                        Ok(false) => {
                                            let _ = send_attachment_superseded(&mut sender).await;
                                            break;
                                        }
                                        Err(error) => {
                                            if send_socket_error(&mut sender, "resize_failed", &error.to_string()).await.is_err() { break; }
                                            active = Some(current);
                                        }
                                    }
                                } else if send_controller_required(&mut sender).await.is_err() {
                                    break;
                                } else {
                                    active = Some(current);
                                }
                            }
                            Err(error) => {
                                if send_socket_error(&mut sender, "invalid_control", &error.to_string()).await.is_err() { break; }
                                active = Some(current);
                            }
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(Message::Ping(_) | Message::Pong(_)) => active = Some(current),
                }
            }
            event = current.events.recv() => {
                match event {
                    Ok(RuntimeEvent::Output { offset, bytes }) => {
                        if offset != current.expected_offset {
                            let _ = send_stream_error(
                                &mut sender,
                                &runtime,
                                current.owner,
                                "output_gap",
                                &format!(
                                    "expected terminal output offset {}, received {offset}; reattach to recover",
                                    current.expected_offset,
                                ),
                            )
                            .await;
                            break;
                        }
                        let byte_count = u64::try_from(bytes.len())
                            .expect("terminal output chunk length fits in u64");
                        let Some(next_offset) = current.expected_offset.checked_add(byte_count) else {
                            let _ = send_stream_error(
                                &mut sender,
                                &runtime,
                                current.owner,
                                "output_gap",
                                "terminal output offset overflow; reattach to recover",
                            )
                            .await;
                            break;
                        };
                        if !send_stream(
                            &mut sender,
                            &runtime,
                            current.owner,
                            Message::Binary(bytes),
                        )
                        .await
                        {
                            break;
                        }
                        current.expected_offset = next_offset;
                        active = Some(current);
                    }
                    Ok(RuntimeEvent::Status { status, exit_code }) => {
                        current.pending_status = Some((status, exit_code));
                        if current.output_closed {
                            let _ = send_stream_status(
                                &mut sender,
                                &runtime,
                                current.owner,
                                status,
                                exit_code,
                            )
                            .await;
                            break;
                        }
                        active = Some(current);
                    }
                    Ok(RuntimeEvent::OutputClosed) => {
                        current.output_closed = true;
                        if let Some((status, exit_code)) = current.pending_status.take() {
                            let _ = send_stream_status(
                                &mut sender,
                                &runtime,
                                current.owner,
                                status,
                                exit_code,
                            )
                            .await;
                            break;
                        }
                        active = Some(current);
                    }
                    Ok(RuntimeEvent::Deleted) => {
                        let _ = send_stream_error(
                            &mut sender,
                            &runtime,
                            current.owner,
                            "terminal_deleted",
                            "terminal runtime was deleted",
                        )
                        .await;
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::debug!(
                            runtime_id = %runtime.id,
                            skipped,
                            expected_offset = current.expected_offset,
                            "terminal attachment lagged; catching up from scrollback"
                        );
                        let Some((expected_offset, events, output_closed)) = catch_up_stream(
                            &mut sender,
                            &runtime,
                            current.owner,
                            current.expected_offset,
                        )
                        .await
                        else {
                            break;
                        };
                        current.expected_offset = expected_offset;
                        current.events = events;
                        current.output_closed |= output_closed;

                        if runtime.is_deleted() {
                            let _ = send_stream_error(
                                &mut sender,
                                &runtime,
                                current.owner,
                                "terminal_deleted",
                                "terminal runtime was deleted",
                            )
                            .await;
                            break;
                        }
                        match runtime.description() {
                            Ok(description) if description.status != TerminalPaneStatus::Running => {
                                current.pending_status =
                                    Some((description.status, description.exit_code));
                            }
                            Ok(_) => {}
                            Err(error) => {
                                let _ = send_stream_error(
                                    &mut sender,
                                    &runtime,
                                    current.owner,
                                    "stream_failed",
                                    &error.to_string(),
                                )
                                .await;
                                break;
                            }
                        }
                        if current.output_closed
                            && let Some((status, exit_code)) = current.pending_status.take()
                        {
                            let _ = send_stream_status(
                                &mut sender,
                                &runtime,
                                current.owner,
                                status,
                                exit_code,
                            )
                            .await;
                            break;
                        }
                        active = Some(current);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            changed = current.controller_changed.changed() => {
                if changed.is_err() {
                    break;
                }
                let changed_owner = *current.controller_changed.borrow_and_update();
                if let Some(owner) = current.owner
                    && changed_owner != Some(owner)
                {
                    let _gate = runtime.delivery_gate.lock().await;
                    if !runtime.is_owner(owner).unwrap_or(false) {
                        let _ = send_attachment_superseded(&mut sender).await;
                        break;
                    }
                }
                active = Some(current);
            }
        }
    }
    if let Some(owner) = held_owner {
        runtime.release(owner);
    }
    let _ = bounded_close(&mut sender).await;
}

pub(super) async fn begin_claim<S>(
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

async fn begin_observation<S>(
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
    initialize_observer_stream(sender, &connection.runtime, observed).await
}

pub(super) async fn initialize_stream<S>(
    sender: &mut S,
    runtime: &Runtime,
    claimed: ClaimedAttachment,
) -> Option<ActiveStream>
where
    S: Sink<Message> + Unpin,
{
    initialize_stream_attachment(sender, runtime, claimed.into()).await
}

async fn initialize_observer_stream<S>(
    sender: &mut S,
    runtime: &Runtime,
    observed: ObservedAttachment,
) -> Option<ActiveStream>
where
    S: Sink<Message> + Unpin,
{
    initialize_stream_attachment(sender, runtime, observed.into()).await
}

async fn initialize_stream_attachment<S>(
    sender: &mut S,
    runtime: &Runtime,
    attachment: StreamAttachment,
) -> Option<ActiveStream>
where
    S: Sink<Message> + Unpin,
{
    let StreamAttachment {
        stream_epoch,
        stream_offset,
        reset,
        replay,
        replay_bytes,
        next_offset,
        owner,
        controller_changed,
        restore,
        restore_cols,
        restore_rows,
    } = attachment;
    let stream = TerminalAttachServerMessage::Stream {
        epoch: stream_epoch,
        offset: stream_offset,
        reset,
        replay_bytes,
        restore,
        restore_cols,
        restore_rows,
    };
    let text = serde_json::to_string(&stream).expect("terminal stream serialization cannot fail");
    if !send_stream(sender, runtime, owner, Message::Text(text.into())).await {
        return None;
    }
    for chunk in replay_chunks(&replay) {
        if !send_stream(
            sender,
            runtime,
            owner,
            Message::Binary(Bytes::copy_from_slice(chunk)),
        )
        .await
        {
            return None;
        }
    }

    // The claim-time broadcast receiver can overflow while an 8 MiB replay is
    // being written. Re-snapshot from the exact byte boundary instead. The
    // same helper also repairs a live receiver that later falls behind.
    let (expected_offset, events, output_closed) =
        catch_up_stream(sender, runtime, owner, next_offset).await?;

    // Events that happened while older receivers were discarded are reflected
    // in these current snapshots of metadata and output-closed state.
    let description = match runtime.description() {
        Ok(description) => description,
        Err(error) => {
            let _ = send_stream_error(sender, runtime, owner, "stream_failed", &error.to_string())
                .await;
            return None;
        }
    };
    let initial_status = description.status;
    let mut pending_status = (initial_status != TerminalPaneStatus::Running)
        .then_some((initial_status, description.exit_code));
    if initial_status == TerminalPaneStatus::Running
        && !send_stream_status(sender, runtime, owner, initial_status, None).await
    {
        return None;
    }
    if output_closed && let Some((status, exit_code)) = pending_status.take() {
        let _ = send_stream_status(sender, runtime, owner, status, exit_code).await;
        return None;
    }
    Some(ActiveStream {
        owner,
        expected_offset,
        output_closed,
        pending_status,
        events,
        controller_changed,
    })
}

/// Recover a stream from an exact raw byte offset without replacing the
/// attachment or asking the browser to reset its emulator. Each pass installs
/// a fresh broadcast receiver under the output lock before sending the copied
/// suffix. Once the suffix is empty, that receiver owns the lossless live
/// boundary. This keeps both initial replay and later broadcast lag on one
/// contiguous stream.
#[cfg(test)]
pub(super) async fn catch_up_output<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: ControllerOwner,
    expected_offset: u64,
) -> Option<(u64, broadcast::Receiver<RuntimeEvent>, bool)>
where
    S: Sink<Message> + Unpin,
{
    catch_up_stream(sender, runtime, Some(owner), expected_offset).await
}

async fn catch_up_stream<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    mut expected_offset: u64,
) -> Option<(u64, broadcast::Receiver<RuntimeEvent>, bool)>
where
    S: Sink<Message> + Unpin,
{
    loop {
        if runtime.is_deleted() {
            let _ = send_stream_error(
                sender,
                runtime,
                owner,
                "terminal_deleted",
                "terminal runtime was deleted",
            )
            .await;
            return None;
        }
        let (snapshot, events) = match runtime.snapshot_and_subscribe(expected_offset) {
            Ok(boundary) => boundary,
            Err(error) => {
                let _ = send_stream_error(sender, runtime, owner, "output_gap", &error.to_string())
                    .await;
                return None;
            }
        };
        // Close the gap between the pre-snapshot deleted check and installing
        // the replacement receiver. A delete after subscribe is retained by
        // `events`; a delete before subscribe is observed here.
        if runtime.is_deleted() {
            let _ = send_stream_error(
                sender,
                runtime,
                owner,
                "terminal_deleted",
                "terminal runtime was deleted",
            )
            .await;
            return None;
        }
        if snapshot.reset {
            let _ = send_stream_error(
                sender,
                runtime,
                owner,
                "output_gap",
                &format!(
                    "terminal output before offset {expected_offset} left the scrollback; reattach to recover"
                ),
            )
            .await;
            return None;
        }
        debug_assert_eq!(snapshot.offset, expected_offset);
        if snapshot.replay.is_empty() {
            return Some((snapshot.next_offset, events, snapshot.closed));
        }
        for chunk in replay_chunks(&snapshot.replay) {
            if runtime.is_deleted() {
                let _ = send_stream_error(
                    sender,
                    runtime,
                    owner,
                    "terminal_deleted",
                    "terminal runtime was deleted",
                )
                .await;
                return None;
            }
            if !send_stream(
                sender,
                runtime,
                owner,
                Message::Binary(Bytes::copy_from_slice(chunk)),
            )
            .await
            {
                return None;
            }
        }
        expected_offset = snapshot.next_offset;
    }
}

pub(super) async fn send_owned<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: ControllerOwner,
    message: Message,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    let _gate = runtime.delivery_gate.lock().await;
    if !runtime.is_owner(owner).unwrap_or(false) {
        let _ = send_attachment_superseded(sender).await;
        return false;
    }
    bounded_send(sender, message).await.is_ok()
}

async fn send_stream<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    message: Message,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    match owner {
        Some(owner) => send_owned(sender, runtime, owner, message).await,
        None => bounded_send(sender, message).await.is_ok(),
    }
}

async fn send_stream_status<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    status: TerminalPaneStatus,
    exit_code: Option<u32>,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    let text = serde_json::to_string(&TerminalAttachServerMessage::Status { status, exit_code })
        .expect("terminal status serialization cannot fail");
    send_stream(sender, runtime, owner, Message::Text(text.into())).await
}

async fn send_stream_error<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    code: &str,
    message: &str,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    let text = socket_error_text(code, message);
    send_stream(sender, runtime, owner, Message::Text(text.into())).await
}

pub(super) fn replay_chunks(replay: &[u8]) -> std::slice::Chunks<'_, u8> {
    replay.chunks(REPLAY_CHUNK_SIZE)
}

pub(super) enum SocketSendError<E> {
    Sink(E),
    Timeout,
}

async fn bounded_send<S>(sender: &mut S, message: Message) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    match timeout(SOCKET_SEND_TIMEOUT, sender.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(SocketSendError::Sink(error)),
        Err(_) => Err(SocketSendError::Timeout),
    }
}

async fn bounded_close<S>(sender: &mut S) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    bounded_close_with_timeout(sender, SOCKET_SEND_TIMEOUT).await
}

pub(super) async fn bounded_close_with_timeout<S>(
    sender: &mut S,
    close_timeout: Duration,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    match timeout(close_timeout, sender.close()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(SocketSendError::Sink(error)),
        Err(_) => Err(SocketSendError::Timeout),
    }
}

async fn send_control<S>(
    sender: &mut S,
    state: TerminalControlState,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    let text = serde_json::to_string(&TerminalAttachServerMessage::Control { state })
        .expect("terminal control serialization cannot fail");
    bounded_send(sender, Message::Text(text.into())).await
}

async fn send_controller_required<S>(sender: &mut S) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    send_socket_error(
        sender,
        "controller_required",
        "claim terminal control before sending input or resize",
    )
    .await
}

async fn send_attachment_superseded<S>(sender: &mut S) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    send_socket_error(
        sender,
        "attachment_superseded",
        "terminal attachment was superseded by a newer controller",
    )
    .await
}

async fn send_resized<S>(
    sender: &mut S,
    cols: u16,
    rows: u16,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    let text = serde_json::to_string(&TerminalAttachServerMessage::Resized { cols, rows })
        .expect("terminal resize serialization cannot fail");
    bounded_send(sender, Message::Text(text.into())).await
}

fn socket_error_text(code: &str, message: &str) -> String {
    serde_json::to_string(&TerminalAttachServerMessage::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    })
    .expect("terminal error serialization cannot fail")
}

async fn send_socket_error<S>(
    sender: &mut S,
    code: &str,
    message: &str,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    bounded_send(
        sender,
        Message::Text(socket_error_text(code, message).into()),
    )
    .await
}
