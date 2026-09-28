use aow_im::{ImKind, Message};
use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use super::{error, update};
use crate::{AppState, notifications::SettingsUpdate};

pub(super) async fn status(State(state): State<AppState>) -> Response {
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

pub(super) async fn start_login(State(state): State<AppState>) -> Response {
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
pub(super) struct PollInput {
    verify_code: Option<String>,
}

pub(super) async fn poll_login(
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

pub(super) async fn cancel_login(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    state.aow.notifications().wechat_login().cancel(&id);
    ([(CACHE_CONTROL, "no-store")], StatusCode::NO_CONTENT).into_response()
}

pub(super) async fn disconnect(State(state): State<AppState>) -> Response {
    state.aow.notifications().wechat_login().cancel_all();
    update(
        &state,
        SettingsUpdate::ImRemove {
            provider: ImKind::Wechat,
        },
    )
    .await
}

pub(super) async fn test_send(State(state): State<AppState>) -> Response {
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
    match client.send_test(&message).await {
        Ok(verification) => (
            [(CACHE_CONTROL, "no-store")],
            Json(json!({"message":"测试消息已发送。", "verification": verification})),
        )
            .into_response(),
        Err(reason) => error(reason),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReceiptInput {
    test_id: String,
    received: bool,
}

pub(super) async fn test_receipt(
    State(state): State<AppState>,
    Json(input): Json<ReceiptInput>,
) -> Response {
    let Some(client) = state.aow.notifications().wechat() else {
        return error("请先扫码绑定微信 Bot");
    };
    match client
        .record_test_receipt(&input.test_id, input.received)
        .await
    {
        Ok(verification) => (
            [(CACHE_CONTROL, "no-store")],
            Json(json!({"verification": verification})),
        )
            .into_response(),
        Err(reason) => error(reason),
    }
}
