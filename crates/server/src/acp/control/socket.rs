use super::*;
use crate::{AppState, auth::SessionAccess};
use axum::{
    Extension,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::{Instant, timeout};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Claim {
        #[serde(default)]
        force: bool,
    },
    Observe,
}

pub(in crate::acp) async fn attach(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
    access: Option<Extension<SessionAccess>>,
    websocket: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    crate::auth::check_origin(&headers)?;
    state
        .acp
        .snapshot(&thread_id)
        .map_err(super::super::api::error)?;
    let controller = state.acp_controls.session(&thread_id);
    Ok(websocket
        .max_message_size(1024)
        .on_upgrade(move |socket| run(socket, controller, access.map(|access| access.0))))
}

async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        timeout(Duration::from_secs(5), socket.send(message)).await,
        Ok(Ok(()))
    )
}

pub(super) async fn run(
    mut socket: WebSocket,
    controller: Arc<Controller>,
    access: Option<SessionAccess>,
) {
    let attachment = Uuid::new_v4();
    let mut changes = controller.changed.subscribe();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut received = Instant::now();
    let revoked = async {
        match &access {
            Some(access) => access.revoked().await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(revoked);
    loop {
        let update = tokio::select! {
            biased;
            _ = &mut revoked => break,
            message = socket.recv() => {
                if access.as_ref().is_some_and(|access| access.validate(false).is_err()) { break; }
                received = Instant::now();
                match message {
                    Some(Ok(Message::Text(text))) => match serde_json::from_str::<Request>(&text) {
                        Ok(Request::Claim { force }) => Some(controller.claim(attachment, force)),
                        Ok(Request::Observe) => { controller.release(attachment); Some(controller.state(attachment)) },
                        Err(_) => break,
                    },
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => None,
                    _ => break,
                }
            },
            changed = changes.changed() => {
                if changed.is_err() { break; }
                // Observers never automatically claim a vacancy or reclaim after takeover.
                Some(controller.state(attachment))
            },
            _ = heartbeat.tick() => {
                if received.elapsed() > Duration::from_secs(35) || !send(&mut socket, Message::Ping(Vec::new().into())).await { break; }
                None
            },
        };
        if let Some(update) = update
            && !send(
                &mut socket,
                Message::Text(serde_json::to_string(&update).unwrap().into()),
            )
            .await
        {
            break;
        }
    }
    controller.release(attachment);
    let _ = send(&mut socket, Message::Close(None)).await;
}
