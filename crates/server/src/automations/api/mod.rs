#![expect(
    clippy::result_large_err,
    reason = "Automation routes return Axum error responses directly without an extra heap allocation"
)]

use super::*;

fn error(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "message": error.to_string() })),
    )
        .into_response()
}

fn manager(state: &AppState) -> Result<&AutomationManager, (StatusCode, Json<serde_json::Value>)> {
    state.automations.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "message": "自动化需要持久化状态目录" })),
        )
    })
}

mod import;
mod runs;
mod tasks;

pub(crate) fn cli_routes() -> Router<AppState> {
    Router::new().route("/v1/automations", post(import::create))
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/automations/status", get(tasks::status))
        .route("/api/aow/automations", get(tasks::list).post(tasks::create))
        .route(
            "/api/aow/automations/{id}",
            get(tasks::detail).put(tasks::update).delete(tasks::remove),
        )
        .route("/api/aow/automations/{id}/enabled", put(tasks::set_enabled))
        .route("/api/aow/automations/{id}/sync", post(tasks::sync))
        .route("/api/aow/automations/{id}/run", post(runs::run))
        .route("/api/aow/automations/{id}/runs", get(runs::runs))
        .route(
            "/api/aow/automations/{id}/runs/{run_id}/output/{output}",
            get(runs::run_output),
        )
        .route(
            "/api/aow/automations/{id}/runs/{run_id}",
            get(runs::run_detail),
        )
}
