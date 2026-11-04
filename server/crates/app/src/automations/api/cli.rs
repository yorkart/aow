//! CLI queries use Server storage even when the caller has no local state directory.
use super::*;
fn error(reason: impl Into<anyhow::Error>) -> Response {
    let error = reason.into();
    let status = match error
        .downcast_ref::<std::io::Error>()
        .map(std::io::Error::kind)
    {
        Some(std::io::ErrorKind::NotFound) => StatusCode::NOT_FOUND,
        Some(std::io::ErrorKind::PermissionDenied) => StatusCode::FORBIDDEN,
        _ => StatusCode::BAD_REQUEST,
    };
    (
        status,
        Json(serde_json::json!({"code":"cli_query_failed","message":error.to_string()})),
    )
        .into_response()
}
use aow_automations::AutomationQuery;
#[derive(Deserialize)]
pub(super) struct Filter {
    project_id: Option<String>,
    #[serde(default)]
    include_deleted: bool,
}
fn query(state: &AppState) -> Result<AutomationQuery, Response> {
    let manager = manager(state).map_err(IntoResponse::into_response)?;
    AutomationQuery::open(manager.store.state_dir.clone()).map_err(error)
}
pub(super) async fn list(
    State(state): State<AppState>,
    Query(filter): Query<Filter>,
) -> Result<Json<serde_json::Value>, Response> {
    Ok(Json(
        serde_json::json!({"items":query(&state)?.tasks(filter.project_id.as_deref(),filter.include_deleted).map_err(error)?}),
    ))
}
pub(super) async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, Response> {
    Ok(Json(
        serde_json::to_value(query(&state)?.task(&id).map_err(error)?).map_err(error)?,
    ))
}
#[derive(Deserialize)]
pub(super) struct Page {
    before: Option<String>,
    #[serde(default = "limit")]
    limit: usize,
}
fn limit() -> usize {
    50
}
pub(super) async fn runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> Result<Json<serde_json::Value>, Response> {
    Ok(Json(
        serde_json::to_value(
            query(&state)?
                .runs(&id, page.before.as_deref(), page.limit.clamp(1, 500))
                .map_err(error)?,
        )
        .map_err(error)?,
    ))
}
pub(super) async fn run(
    State(state): State<AppState>,
    Path((id, run)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, Response> {
    Ok(Json(
        serde_json::to_value(query(&state)?.run(&id, &run).map_err(error)?).map_err(error)?,
    ))
}
