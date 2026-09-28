use super::attachment::ActiveStream;
use super::replay::{catch_up_stream, replay_chunks};
use super::*;

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

pub(in crate::runtime) async fn initialize_stream<S>(
    sender: &mut S,
    runtime: &Runtime,
    claimed: ClaimedAttachment,
) -> Option<ActiveStream>
where
    S: Sink<Message> + Unpin,
{
    initialize_stream_attachment(sender, runtime, claimed.into()).await
}

pub(super) async fn initialize_observer_stream<S>(
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
