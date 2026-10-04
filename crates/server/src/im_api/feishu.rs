use aow_im::{ImConfigUpdate, ImKind};
use axum::{Json, extract::State, response::Response};
use serde::Deserialize;

use super::update;
use crate::{AppState, notifications::SettingsUpdate};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FeishuInput {
    app_id: String,
    app_secret: Option<String>,
}

pub(super) async fn save_feishu(
    State(state): State<AppState>,
    Json(input): Json<FeishuInput>,
) -> Response {
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

pub(super) async fn remove_feishu(State(state): State<AppState>) -> Response {
    update(
        &state,
        SettingsUpdate::ImRemove {
            provider: ImKind::Feishu,
        },
    )
    .await
}
