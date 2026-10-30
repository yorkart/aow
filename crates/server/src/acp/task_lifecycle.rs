//! Task-owned session cleanup uses the same page ownership checks as ordinary ACP deletion.
use crate::{AppState, HttpError};
use axum::http::HeaderMap;

pub(crate) async fn delete_task_session(
    state: &AppState,
    thread: &str,
    headers: &HeaderMap,
) -> Result<(), HttpError> {
    let controller = state.acp_controls.session(thread);
    let _lifecycle = controller.authorize_lifecycle(headers, true).await?;
    state
        .acp
        .delete_session(thread)
        .await
        .map_err(super::api::error)
}
