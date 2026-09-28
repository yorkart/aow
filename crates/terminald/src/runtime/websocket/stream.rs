use futures_util::Sink;
use tokio::sync::broadcast;

use super::attachment::ActiveStream;
use super::*;

pub(super) async fn handle_runtime_event<S>(
    sender: &mut S,
    runtime: &Runtime,
    mut current: ActiveStream,
    event: Result<RuntimeEvent, broadcast::error::RecvError>,
) -> Option<ActiveStream>
where
    S: Sink<Message> + Unpin,
{
    match event {
        Ok(RuntimeEvent::Output { offset, bytes }) => {
            if offset != current.expected_offset {
                let _ = send_stream_error(
                    sender,
                    runtime,
                    current.owner,
                    "output_gap",
                    &format!(
                        "expected terminal output offset {}, received {offset}; reattach to recover",
                        current.expected_offset,
                    ),
                )
                .await;
                return None;
            }
            let byte_count =
                u64::try_from(bytes.len()).expect("terminal output chunk length fits in u64");
            let Some(next_offset) = current.expected_offset.checked_add(byte_count) else {
                let _ = send_stream_error(
                    sender,
                    runtime,
                    current.owner,
                    "output_gap",
                    "terminal output offset overflow; reattach to recover",
                )
                .await;
                return None;
            };
            if !send_stream(sender, runtime, current.owner, Message::Binary(bytes)).await {
                return None;
            }
            current.expected_offset = next_offset;
            Some(current)
        }
        Ok(RuntimeEvent::Status { status, exit_code }) => {
            current.pending_status = Some((status, exit_code));
            if current.output_closed {
                let _ = send_stream_status(sender, runtime, current.owner, status, exit_code).await;
                None
            } else {
                Some(current)
            }
        }
        Ok(RuntimeEvent::OutputClosed) => {
            current.output_closed = true;
            if let Some((status, exit_code)) = current.pending_status.take() {
                let _ = send_stream_status(sender, runtime, current.owner, status, exit_code).await;
                None
            } else {
                Some(current)
            }
        }
        Ok(RuntimeEvent::Deleted) => {
            let _ = send_stream_error(
                sender,
                runtime,
                current.owner,
                "terminal_deleted",
                "terminal runtime was deleted",
            )
            .await;
            None
        }
        Err(broadcast::error::RecvError::Lagged(skipped)) => {
            tracing::debug!(
                runtime_id = %runtime.id,
                skipped,
                expected_offset = current.expected_offset,
                "terminal attachment lagged; catching up from scrollback"
            );
            let Some((expected_offset, events, output_closed)) =
                catch_up_stream(sender, runtime, current.owner, current.expected_offset).await
            else {
                return None;
            };
            current.expected_offset = expected_offset;
            current.events = events;
            current.output_closed |= output_closed;

            if runtime.is_deleted() {
                let _ = send_stream_error(
                    sender,
                    runtime,
                    current.owner,
                    "terminal_deleted",
                    "terminal runtime was deleted",
                )
                .await;
                return None;
            }
            match runtime.description() {
                Ok(description) if description.status != TerminalPaneStatus::Running => {
                    current.pending_status = Some((description.status, description.exit_code));
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = send_stream_error(
                        sender,
                        runtime,
                        current.owner,
                        "stream_failed",
                        &error.to_string(),
                    )
                    .await;
                    return None;
                }
            }
            if current.output_closed
                && let Some((status, exit_code)) = current.pending_status.take()
            {
                let _ = send_stream_status(sender, runtime, current.owner, status, exit_code).await;
                return None;
            }
            Some(current)
        }
        Err(broadcast::error::RecvError::Closed) => None,
    }
}
