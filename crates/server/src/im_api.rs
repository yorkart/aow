//! Authenticated HTTP boundary only; all IM protocol/auth/session logic is in aow-im.
use aow_im::{ImConfigUpdate, ImKind, ImProvider, Message};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::json;

use crate::{AppState, notifications::SettingsUpdate};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/im/feishu", put(save_feishu).delete(remove_feishu))
        .route("/api/aow/im/wechat", get(status).delete(disconnect))
        .route("/api/aow/im/wechat/login", post(start_login))
        // POST because confirmation persists credentials; verification codes stay out of URLs.
        .route(
            "/api/aow/im/wechat/login/{id}",
            post(poll_login).delete(cancel_login),
        )
        .route("/api/aow/im/wechat/test", post(test_send))
}

fn error(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(CACHE_CONTROL, "no-store")],
        Json(json!({"message":error.to_string()})),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeishuInput {
    app_id: String,
    app_secret: Option<String>,
}

async fn save_feishu(State(state): State<AppState>, Json(input): Json<FeishuInput>) -> Response {
    update(
        &state,
        SettingsUpdate::ImProvider {
            config: ImConfigUpdate::Feishu {
                app_id: input.app_id,
                app_secret: input.app_secret,
            },
        },
    )
    .await
}

async fn remove_feishu(State(state): State<AppState>) -> Response {
    update(
        &state,
        SettingsUpdate::ImRemove {
            provider: ImKind::Feishu,
        },
    )
    .await
}

async fn update(state: &AppState, input: SettingsUpdate) -> Response {
    let manager = state.aow.notifications().clone();
    match tokio::task::spawn_blocking(move || manager.update(input)).await {
        Ok(Ok(view)) => ([(CACHE_CONTROL, "no-store")], Json(view)).into_response(),
        Ok(Err(reason)) => error(reason),
        Err(_) => error("无法保存 IM 配置"),
    }
}

async fn status(State(state): State<AppState>) -> Response {
    let connection = state
        .aow
        .notifications()
        .wechat()
        .map(|client| client.status());
    (
        [(CACHE_CONTROL, "no-store")],
        Json(json!({"connection":connection})),
    )
        .into_response()
}

async fn start_login(State(state): State<AppState>) -> Response {
    let manager = state.aow.notifications();
    match manager
        .wechat_login()
        .start(manager.wechat_credentials())
        .await
    {
        Ok(view) => ([(CACHE_CONTROL, "no-store")], Json(view)).into_response(),
        Err(reason) => error(reason),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PollInput {
    verify_code: Option<String>,
}

async fn poll_login(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<PollInput>,
) -> Response {
    let manager = state.aow.notifications();
    match manager
        .wechat_login()
        .poll(&id, input.verify_code.as_deref(), |credentials| {
            // The login manager holds its short commit lock, protecting cancellation
            // and rebind races. This local atomic save occurs only on confirmation.
            manager.update(SettingsUpdate::WechatBinding { credentials })?;
            Ok(())
        })
        .await
    {
        Ok(view) => ([(CACHE_CONTROL, "no-store")], Json(view)).into_response(),
        Err(reason) => error(reason),
    }
}

async fn cancel_login(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    state.aow.notifications().wechat_login().cancel(&id);
    ([(CACHE_CONTROL, "no-store")], StatusCode::NO_CONTENT).into_response()
}

async fn disconnect(State(state): State<AppState>) -> Response {
    state.aow.notifications().wechat_login().cancel_all();
    update(
        &state,
        SettingsUpdate::ImRemove {
            provider: ImKind::Wechat,
        },
    )
    .await
}

async fn test_send(State(state): State<AppState>) -> Response {
    let Some(client) = state.aow.notifications().wechat() else {
        return error("请先扫码绑定微信 Bot");
    };
    let message = Message {
        title: "AoW 微信推送测试".into(),
        fields: vec![],
        body_label: "测试消息".into(),
        body: "微信通知已连通。可在 Settings → 通知中启用微信推送。".into(),
        markdown: false,
        error: false,
    };
    match client
        .send(&message, &uuid::Uuid::new_v4().to_string())
        .await
    {
        Ok(()) => (
            [(CACHE_CONTROL, "no-store")],
            Json(json!({"message":"测试消息已发送。"})),
        )
            .into_response(),
        Err(reason) => error(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn all_im_routes_require_existing_pin_authentication() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(crate::auth::PIN_HASH_FILE),
            format!("{:x}", md5::compute("123456")),
        )
        .unwrap();
        let mut state = AppState::new(root.path().join("frontend"));
        state.auth = crate::auth::PinAuth::persistent(root.path());
        let app = crate::build_router(state);
        for (method, path) in [
            ("PUT", "/api/aow/im/feishu"),
            ("DELETE", "/api/aow/im/feishu"),
            ("GET", "/api/aow/im/wechat"),
            ("DELETE", "/api/aow/im/wechat"),
            ("POST", "/api/aow/im/wechat/login"),
            ("POST", "/api/aow/im/wechat/login/id"),
            ("DELETE", "/api/aow/im/wechat/login/id"),
            ("POST", "/api/aow/im/wechat/test"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {path}"
            );
        }
    }

    #[tokio::test]
    async fn provider_api_keeps_secrets_private_and_errors_are_not_cached() {
        let app = crate::build_router(AppState::new("/unused".into()));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/aow/im/feishu")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"app_id":"cli_test","app_secret":"private-secret"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        let body = to_bytes(response.into_body(), 10000).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("private-secret"));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/aow/im/wechat/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
    }
}
