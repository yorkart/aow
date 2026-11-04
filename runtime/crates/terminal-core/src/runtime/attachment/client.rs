use futures_util::Sink;
use std::sync::Arc;

use super::ownership::ActiveStream;
use super::*;

pub(super) async fn handle_client_message<S>(
    sender: &mut S,
    connection: &RuntimeConnection,
    runtime: &Arc<Runtime>,
    held_owner: &mut Option<ControllerOwner>,
    controlled: bool,
    current: ActiveStream,
    message: TerminalInput,
) -> Option<ActiveStream>
where
    S: Sink<TerminalEvent> + Unpin,
{
    match message {
        TerminalInput::Data(bytes) => {
            if let Some(owner) = current.owner {
                let _gate = runtime.delivery_gate.lock().await;
                let write_runtime = runtime.clone();
                let result =
                    tokio::task::spawn_blocking(move || write_runtime.write_input(owner, &bytes))
                        .await;
                match result {
                    Ok(Ok(true)) => Some(current),
                    Ok(Ok(false)) => {
                        let _ = send_attachment_superseded(sender).await;
                        None
                    }
                    Ok(Err(error)) => {
                        if send_attachment_error(sender, "input_failed", &error.to_string())
                            .await
                            .is_err()
                        {
                            None
                        } else {
                            Some(current)
                        }
                    }
                    Err(error) => {
                        if send_attachment_error(sender, "input_failed", &error.to_string())
                            .await
                            .is_err()
                        {
                            None
                        } else {
                            Some(current)
                        }
                    }
                }
            } else if send_controller_required(sender).await.is_err() {
                None
            } else {
                Some(current)
            }
        }
        TerminalInput::Command(command) => match command {
            TerminalAttachClientMessage::Observe => {
                if let Some(owner) = held_owner.take() {
                    runtime.release(owner);
                }
                begin_observation(sender, connection).await
            }
            TerminalAttachClientMessage::Submit { request_id, task } => {
                if request_id.is_empty() || request_id.len() > 128 {
                    let _ =
                        send_attachment_error(sender, "invalid_input", "Invalid request ID").await;
                    return None;
                }
                if let Some(owner) = current.owner {
                    let result = tokio::time::timeout(
                        Duration::from_secs(15),
                        super::super::agent_operations::submit_owned(runtime, owner, &task),
                    )
                    .await;
                    match result {
                        Ok(Ok(())) => {
                            let message = TerminalAttachServerMessage::Written { request_id };
                            if bounded_send(sender, TerminalEvent::Message(message))
                                .await
                                .is_err()
                            {
                                return None;
                            }
                        }
                        Ok(Err(error)) => {
                            let _ = send_attachment_error(
                                sender,
                                "submission_failed",
                                &error.to_string(),
                            )
                            .await;
                            return None;
                        }
                        Err(_) => {
                            let _ = send_attachment_error(
                                sender,
                                "submission_timeout",
                                "Submission timed out; delivery status is unknown",
                            )
                            .await;
                            return None;
                        }
                    }
                } else if send_controller_required(sender).await.is_err() {
                    return None;
                }
                Some(current)
            }
            TerminalAttachClientMessage::Write { request_id, data } => {
                if request_id.len() > 128 || data.len() > 256 * 1024 {
                    if send_attachment_error(sender, "invalid_input", "input exceeds size limit")
                        .await
                        .is_err()
                    {
                        return None;
                    }
                } else if let Some(owner) = current.owner {
                    let _gate = runtime.delivery_gate.lock().await;
                    let write_runtime = runtime.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        write_runtime.write_input(owner, data.as_bytes())
                    })
                    .await;
                    match result {
                        Ok(Ok(true)) => {
                            let message = TerminalAttachServerMessage::Written { request_id };
                            if bounded_send(sender, TerminalEvent::Message(message))
                                .await
                                .is_err()
                            {
                                return None;
                            }
                        }
                        Ok(Ok(false)) => {
                            let _ = send_attachment_superseded(sender).await;
                            return None;
                        }
                        _ => {
                            let _ =
                                send_attachment_error(sender, "input_failed", "PTY input failed")
                                    .await;
                            return None;
                        }
                    }
                } else if send_controller_required(sender).await.is_err() {
                    return None;
                }
                Some(current)
            }
            TerminalAttachClientMessage::Claim { force } => {
                match begin_claim(sender, connection, force, controlled, current.owner, false).await
                {
                    BeginClaim::Claimed(claimed) => {
                        *held_owner = Some(claimed.owner);
                        initialize_stream(sender, runtime, claimed).await
                    }
                    BeginClaim::Waiting | BeginClaim::Unchanged => Some(current),
                    BeginClaim::End => None,
                }
            }
            TerminalAttachClientMessage::Resize { cols, rows } => {
                if let Some(owner) = current.owner {
                    let _gate = runtime.delivery_gate.lock().await;
                    match runtime.resize(owner, rows, cols) {
                        Ok(true) => {
                            if send_resized(sender, cols, rows).await.is_err() {
                                return None;
                            }
                            Some(current)
                        }
                        Ok(false) => {
                            let _ = send_attachment_superseded(sender).await;
                            None
                        }
                        Err(error) => {
                            if send_attachment_error(sender, "resize_failed", &error.to_string())
                                .await
                                .is_err()
                            {
                                None
                            } else {
                                Some(current)
                            }
                        }
                    }
                } else if send_controller_required(sender).await.is_err() {
                    None
                } else {
                    Some(current)
                }
            }
        },
        TerminalInput::InvalidCommand(error) => {
            if send_attachment_error(sender, "invalid_control", &error)
                .await
                .is_err()
            {
                None
            } else {
                Some(current)
            }
        }
    }
}
