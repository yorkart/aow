use axum::{
    Json, Router,
    http::{StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde_json::json;

use crate::{AppState, notifications::SettingsUpdate};

mod feishu;
mod wechat;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/aow/im/feishu",
            put(feishu::save_feishu).delete(feishu::remove_feishu),
        )
        .route(
            "/api/aow/im/wechat",
            get(wechat::status).delete(wechat::disconnect),
        )
        .route("/api/aow/im/wechat/login", post(wechat::start_login))
        // POST because confirmation persists credentials; verification codes stay out of URLs.
        .route(
            "/api/aow/im/wechat/login/{id}",
            post(wechat::poll_login).delete(wechat::cancel_login),
        )
        .route("/api/aow/im/wechat/test", post(wechat::test_send))
        .route("/api/aow/im/wechat/test/receipt", put(wechat::test_receipt))
}

fn error(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(CACHE_CONTROL, "no-store")],
        Json(json!({"message":error.to_string()})),
    )
        .into_response()
}

async fn update(state: &AppState, input: SettingsUpdate) -> Response {
    let manager = state.aow.notifications().clone();
    match tokio::task::spawn_blocking(move || manager.update(input)).await {
        Ok(Ok(view)) => ([(CACHE_CONTROL, "no-store")], Json(view)).into_response(),
        Ok(Err(reason)) => error(reason),
        Err(_) => error("无法保存 IM 配置"),
    }
}

#[cfg(test)]
mod tests;
