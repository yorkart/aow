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
            Json(serde_json::json!({ "message": aow_i18n::text("server.automations_require_a_persistent_state_directory") })),
        )
    })
}

mod cli;
mod import;
mod runs;
mod tasks;

pub(crate) fn cli_routes() -> Router<AppState> {
    Router::new()
        .route("/v1/automations", get(cli::list).post(import::create))
        .route("/v1/automations/{id}", get(cli::get))
        .route("/v1/automations/{id}/runs", get(cli::runs))
        .route("/v1/automations/{id}/runs/{run}", get(cli::run))
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/automations/status", get(tasks::status))
        .route("/api/automations", get(tasks::list).post(tasks::create))
        .route(
            "/api/automations/{id}",
            get(tasks::detail).put(tasks::update).delete(tasks::remove),
        )
        .route("/api/automations/{id}/enabled", put(tasks::set_enabled))
        .route("/api/automations/{id}/sync", post(tasks::sync))
        .route("/api/automations/{id}/run", post(runs::run))
        .route("/api/automations/{id}/runs", get(runs::runs))
        .route(
            "/api/automations/{id}/runs/{run_id}/output/{output}",
            get(runs::run_output),
        )
        .route("/api/automations/{id}/runs/{run_id}", get(runs::run_detail))
}
