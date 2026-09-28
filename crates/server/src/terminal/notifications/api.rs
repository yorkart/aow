use std::convert::Infallible;

use axum::{
    extract::State,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};

use crate::AppState;

pub(in crate::terminal) async fn events(State(state): State<AppState>) -> impl IntoResponse {
    let receiver = state.terminals.inner.task_stops.subscribe();
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(notification) => {
                    let event = Event::default()
                        .event("task-stopped")
                        .json_data(notification)
                        .expect("serializable notification");
                    return Some((Ok::<_, Infallible>(event), receiver));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    (
        [("X-Accel-Buffering", "no")],
        Sse::new(stream).keep_alive(KeepAlive::default()),
    )
}
