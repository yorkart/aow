use super::*;

pub(super) async fn attach_terminal_pane(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
    Query(query): Query<AttachQuery>,
    headers: HeaderMap,
    access: Option<axum::Extension<crate::auth::SessionAccess>>,
    websocket: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    super::origin::validate_request_origin(&headers).map_err(terminal_http_error)?;
    state
        .terminals
        .ensure_pane(&tab_id, &pane_id)
        .map_err(terminal_http_error)?;
    let manager = state.terminals.clone();
    Ok(websocket
        .on_upgrade(move |socket| async move {
            match manager
                .attach(
                    &tab_id,
                    &pane_id,
                    AttachOptions {
                        epoch: query.epoch.as_deref(),
                        after: query.after,
                        controlled: query.control.as_deref() == Some("v2"),
                        vt_snapshot: query.capabilities.as_deref() == Some("vt-snapshot-v1"),
                        observer: query.observer.as_deref() == Some("v1"),
                    },
                )
                .await
            {
                Ok(daemon) => {
                    super::super::bridge::bridge_terminal_socket(
                        socket,
                        daemon,
                        manager,
                        pane_id,
                        access.map(|access| access.0),
                    )
                    .await
                }
                Err(error) => reject_terminal_socket(socket, error).await,
            }
        })
        .into_response())
}

pub(super) async fn reject_terminal_socket(mut browser: WebSocket, error: TerminalError) {
    let text = serde_json::to_string(&TerminalAttachServerMessage::Error {
        code: "attach_failed".to_owned(),
        message: error.to_string(),
    })
    .expect("terminal attach error serialization cannot fail");
    let _ = browser.send(Message::Text(text.into())).await;
    let _ = browser.close().await;
}
