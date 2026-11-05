//! Native session HTTP routes; lookup and persistence belong to agents-core.
use aow_agents_core::catalog::{self, SessionCatalog, SessionError, SessionErrorKind};
use aow_host_contract::agents::{
    NativePaneRequest, NativeProcessCheck, NativeSessionQuery, ResolveSessionWorktree,
};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};

struct HttpError(SessionError);
impl From<SessionError> for HttpError {
    fn from(value: SessionError) -> Self {
        Self(value)
    }
}
impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = match self.0.kind {
            SessionErrorKind::Invalid => StatusCode::BAD_REQUEST,
            SessionErrorKind::NotFound => StatusCode::NOT_FOUND,
            SessionErrorKind::Unavailable => StatusCode::GONE,
            SessionErrorKind::Conflict => StatusCode::CONFLICT,
            SessionErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(serde_json::json!({"code":"node_session_error","message":self.0.to_string()})),
        )
            .into_response()
    }
}
pub fn router(catalog: SessionCatalog) -> Router {
    Router::new()
        .route("/v1/agent-sessions/list", post(list))
        .route("/v1/agent-sessions/find", post(find))
        .route("/v1/agent-sessions/snapshot", post(snapshot))
        .route("/v1/agent-sessions/pane", post(read_pane))
        .route("/v1/agent-sessions/process-check", post(process_check))
        .route(
            "/v1/agent-sessions/resolve-worktree",
            post(resolve_worktree),
        )
        .with_state(catalog)
}

async fn list(
    State(catalog): State<SessionCatalog>,
    Json(request): Json<NativeSessionQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(catalog::list(catalog, request).await?))
}

async fn find(
    State(catalog): State<SessionCatalog>,
    Json(request): Json<NativeSessionQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(catalog::find(catalog, request).await?))
}

async fn snapshot(
    State(catalog): State<SessionCatalog>,
    Json(request): Json<NativeSessionQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(catalog::snapshot(catalog, request).await?))
}

async fn read_pane(
    State(catalog): State<SessionCatalog>,
    Json(request): Json<NativePaneRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(catalog::read_pane(catalog, request).await?))
}

async fn process_check(
    State(catalog): State<SessionCatalog>,
    Json(request): Json<NativeProcessCheck>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(catalog::process_check(catalog, request).await?))
}

async fn resolve_worktree(
    Json(request): Json<ResolveSessionWorktree>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(catalog::resolve_worktree(request).await?))
}
