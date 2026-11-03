//! WebSocket attachment, replay, and controller hand-off.

use super::*;

mod attachment;
mod client;
mod initialization;
mod replay;
mod send;
mod stream;
mod waiting;

use attachment::begin_observation;
pub(super) use attachment::{BeginClaim, begin_claim};
use client::handle_client_message;
pub(super) use initialization::initialize_stream;
use replay::catch_up_stream;
#[cfg(test)]
pub(super) use replay::{catch_up_output, replay_chunks};
#[cfg(test)]
pub(super) use send::{SocketSendError, bounded_close_with_timeout, send_owned};
use send::{
    bounded_close, bounded_send, send_attachment_superseded, send_control,
    send_controller_required, send_resized, send_socket_error, send_stream, send_stream_error,
    send_stream_status,
};
use stream::handle_runtime_event;
use waiting::handle_waiting_attachment_event;

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
            if !handle_waiting_attachment_event(
                &mut sender,
                &mut receiver,
                &mut connection,
                &runtime,
                &mut active,
                &mut held_owner,
                &mut waiting,
                observe,
            )
            .await
            {
                break;
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
                let Some(Ok(message)) = incoming else { break };
                let Some(next) = handle_client_message(
                    &mut sender,
                    &connection,
                    &runtime,
                    &mut held_owner,
                    controlled,
                    current,
                    message,
                )
                .await else {
                    break;
                };
                active = Some(next);
            }
            event = current.events.recv() => {
                let Some(next) = handle_runtime_event(&mut sender, &runtime, current, event).await else {
                    break;
                };
                active = Some(next);
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
