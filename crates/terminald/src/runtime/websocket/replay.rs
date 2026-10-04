use futures_util::Sink;
use tokio::sync::broadcast;

use super::*;

/// Recover a stream from an exact raw byte offset without replacing the
/// attachment or asking the browser to reset its emulator. Each pass installs
/// a fresh broadcast receiver under the output lock before sending the copied
/// suffix. Once the suffix is empty, that receiver owns the lossless live
/// boundary. This keeps both initial replay and later broadcast lag on one
/// contiguous stream.
#[cfg(test)]
pub(in crate::runtime) async fn catch_up_output<S>(
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

pub(super) async fn catch_up_stream<S>(
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

pub(in crate::runtime) fn replay_chunks(replay: &[u8]) -> std::slice::Chunks<'_, u8> {
    replay.chunks(REPLAY_CHUNK_SIZE)
}
