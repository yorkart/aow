use futures_util::{Sink, Stream, StreamExt};

use super::attachment::ActiveStream;
use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "Pass both socket halves and attachment state explicitly during the waiting phase"
)]
pub(super) async fn handle_waiting_attachment_event<S, R, E>(
    sender: &mut S,
    receiver: &mut R,
    connection: &mut RuntimeConnection,
    runtime: &Runtime,
    active: &mut Option<ActiveStream>,
    held_owner: &mut Option<ControllerOwner>,
    waiting: &mut bool,
    observe: bool,
) -> bool
where
    S: Sink<Message> + Unpin,
    R: Stream<Item = Result<Message, E>> + Unpin,
{
    tokio::select! {
        incoming = receiver.next() => {
            let Some(incoming) = incoming else { return false; };
            match incoming {
                Ok(Message::Binary(_)) => {
                    if send_controller_required(sender).await.is_err() { return false; }
                }
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<TerminalAttachClientMessage>(&text) {
                        Ok(TerminalAttachClientMessage::Observe) => {
                            *waiting = false;
                            *active = begin_observation(sender, connection).await;
                            if active.is_none() { return false; }
                        }
                        Ok(TerminalAttachClientMessage::Claim { force }) => {
                            match begin_claim(sender, connection, force, true, None, observe).await {
                                BeginClaim::Claimed(claimed) => {
                                    *waiting = false;
                                    *held_owner = Some(claimed.owner);
                                    *active = initialize_stream(sender, runtime, claimed).await;
                                    if active.is_none() { return false; }
                                }
                                BeginClaim::Waiting if observe => {
                                    *active = begin_observation(sender, connection).await;
                                    if active.is_none() { return false; }
                                }
                                BeginClaim::Waiting => *waiting = true,
                                BeginClaim::Unchanged => {}
                                BeginClaim::End => return false,
                            }
                        }
                        Ok(TerminalAttachClientMessage::Resize { .. } | TerminalAttachClientMessage::Write { .. }) => {
                            if send_controller_required(sender).await.is_err() { return false; }
                        }
                        Err(error) => {
                            if send_socket_error(sender, "invalid_control", &error.to_string()).await.is_err() { return false; }
                        }
                    }
                }
                Ok(Message::Close(_)) | Err(_) => return false,
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
            }
        }
        changed = connection.deleted_changed.changed() => {
            if changed.is_err() || *connection.deleted_changed.borrow_and_update() {
                let _ = send_socket_error(sender, "terminal_deleted", "terminal runtime was deleted").await;
                return false;
            }
        }
        changed = connection.controller_changed.changed(), if *waiting => {
            if changed.is_err() {
                return false;
            }
            connection.controller_changed.borrow_and_update();
            // `watch` may coalesce a fast vacant -> newly-owned pair.
            // Retrying on every revision lets exactly one waiter claim
            // the mutex-protected vacancy while every loser receives a
            // fresh waiting response and remains eligible for the next
            // release.
            match begin_claim(sender, connection, false, true, None, false).await {
                BeginClaim::Claimed(claimed) => {
                    *waiting = false;
                    *held_owner = Some(claimed.owner);
                    *active = initialize_stream(sender, runtime, claimed).await;
                    if active.is_none() { return false; }
                }
                BeginClaim::Waiting => {}
                BeginClaim::Unchanged => {}
                BeginClaim::End => return false,
            }
        }
    }
    true
}
