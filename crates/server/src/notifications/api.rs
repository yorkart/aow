use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header::CACHE_CONTROL},
    routing::get,
};

use crate::{AppState, HttpError};

use super::SettingsUpdate;

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/aow/notification-settings",
        get(get_settings).put(update_settings),
    )
}

async fn get_settings(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    (
        [(CACHE_CONTROL, "no-store")],
        Json(state.aow.notifications().view()),
    )
}

async fn update_settings(
    State(state): State<AppState>,
    Json(update): Json<SettingsUpdate>,
) -> Result<impl axum::response::IntoResponse, HttpError> {
    let manager = state.aow.notifications().clone();
    let settings = tokio::task::spawn_blocking(move || manager.update(update))
        .await
        .map_err(|_| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "notification_settings_failed",
                "无法保存通知配置",
                None,
            )
        })?
        .map_err(crate::aow::aow_http_error)?;
    Ok(([(CACHE_CONTROL, "no-store")], Json(settings)))
}
