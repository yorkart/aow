use aow_agents::sessions::snapshot::AgentSessionSnapshot;
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use serde::Deserialize;

use super::{
    PUBLIC_API_PATH,
    store::{ShareInfo, lock_error, read_snapshot},
};
use crate::{AppState, HttpError, aow};

#[derive(Deserialize)]
struct ShareQuery {
    agent: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateShare {
    agent: String,
    worktree_path: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/aow/agent-sessions/{session_id}/share",
            get(info).post(create),
        )
        .route("/api/aow/session-shares/{id}", delete(revoke))
        .route(PUBLIC_API_PATH, get(read))
        .route("/share/{token}", get(page))
        .route("/share/{token}/", get(page))
        .layer(axum::middleware::map_response(share_headers))
}

async fn share_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-robots-tag",
        HeaderValue::from_static("noindex, nofollow"),
    );
    response
}

async fn page(State(state): State<AppState>) -> Result<Response, HttpError> {
    crate::serve_spa_index(&state).await
}

async fn info(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ShareQuery>,
) -> Result<Json<Option<ShareInfo>>, HttpError> {
    state
        .session_shares
        .find(&query.agent, &session_id)
        .map(|info| {
            info.map(|mut info| {
                info.path = state.base_path.url(&info.path);
                info
            })
        })
        .map(Json)
}

async fn create(
    State(state): State<AppState>,
    AxumPath(session_id): AxumPath<String>,
    Json(input): Json<CreateShare>,
) -> Result<Json<ShareInfo>, Response> {
    let locator =
        aow::resolve_session_locator(&state, &session_id, &input.agent, &input.worktree_path)
            .await?;
    // Check availability before publishing; never accept a client-supplied transcript path.
    read_snapshot(locator.clone())
        .await
        .map_err(IntoResponse::into_response)?;
    let base_path = state.base_path.clone();
    tokio::task::spawn_blocking(move || state.session_shares.create(locator))
        .await
        .map_err(|_| lock_error().into_response())?
        .map(|mut info| {
            info.path = base_path.url(&info.path);
            Json(info)
        })
        .map_err(IntoResponse::into_response)
}

async fn revoke(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, HttpError> {
    tokio::task::spawn_blocking(move || state.session_shares.revoke(&id))
        .await
        .map_err(|_| lock_error())??;
    Ok(StatusCode::NO_CONTENT)
}

async fn read(
    State(state): State<AppState>,
    AxumPath(token): AxumPath<String>,
) -> Result<Json<AgentSessionSnapshot>, HttpError> {
    state.session_shares.read(&token).await.map(Json)
}
