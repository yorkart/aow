use crate::{AppState, HttpError};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header::CACHE_CONTROL},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/zed/settings", get(settings).put(save_settings))
        .route(
            "/api/zed/connections/{id}/requests",
            get(connection_requests).post(answer_connection),
        )
        .route("/api/zed/agents", get(agents))
        .route(
            "/api/zed/agents/{id}/installation",
            get(installation_status).post(install_agent),
        )
        .route("/api/zed/registry", get(registry))
        .route("/api/zed/connections", post(connect))
        .route("/api/zed/connection-status", get(connection_status))
        .route(
            "/api/zed/connections/{id}",
            axum::routing::delete(disconnect),
        )
        .route("/api/zed/connections/{id}/authenticate", post(authenticate))
        .route(
            "/api/zed/connections/{id}/sessions",
            get(remote_sessions).post(new_session),
        )
        .route("/api/zed/connections/{id}/load", post(load_session))
        .route("/api/zed/connections/{id}/import", post(import_sessions))
        .route("/api/zed/connections/{id}/logs", get(connection_logs))
        .route("/api/zed/sessions", get(sessions))
        .route(
            "/api/zed/sessions/{id}",
            get(snapshot).delete(delete_session),
        )
        .route("/api/zed/sessions/{id}/actions", post(session_action))
        .route("/api/zed/sessions/{id}/logs", get(session_logs))
        .layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response
                    .headers_mut()
                    .insert(CACHE_CONTROL, "no-store".parse().unwrap());
                response
            },
        ))
}
fn error(error: anyhow::Error) -> HttpError {
    HttpError::new(
        StatusCode::BAD_REQUEST,
        if aow_zed::is_auth_required(&error) {
            "acp_auth_required"
        } else {
            "acp_error"
        },
        format!("{error:#}"),
        None,
    )
}
async fn settings(
    State(state): State<AppState>,
) -> Result<Json<crate::aow::ZedSettingsFile>, HttpError> {
    tokio::task::spawn_blocking(move || state.aow.zed_settings())
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?
        .map(Json)
        .map_err(error)
}
#[derive(Deserialize)]
struct SaveSettings {
    content: String,
    revision: String,
}
async fn save_settings(
    State(state): State<AppState>,
    Json(request): Json<SaveSettings>,
) -> Result<Json<crate::aow::ZedSettingsFile>, HttpError> {
    state
        .aow
        .save_zed_settings(request.content, request.revision)
        .await
        .map(Json)
        .map_err(error)
}
async fn agents(State(state): State<AppState>) -> Result<Json<Vec<aow_zed::AgentInfo>>, HttpError> {
    state.zed.agents().await.map(Json).map_err(error)
}
async fn install_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.zed.install_agent(&id).await.map_err(error)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn installation_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Option<aow_zed::ConnectionStatus>> {
    Json(state.zed.installation_status(&id))
}
#[derive(Default, Deserialize)]
struct RegistryQuery {
    #[serde(default)]
    refresh: bool,
}
async fn registry(
    State(state): State<AppState>,
    Query(query): Query<RegistryQuery>,
) -> Result<Json<Vec<aow_zed::AgentInfo>>, HttpError> {
    state
        .zed
        .registry(query.refresh)
        .await
        .map(Json)
        .map_err(error)
}
#[derive(Deserialize)]
struct Connect {
    agent_id: String,
    cwd: std::path::PathBuf,
}
async fn connection_status(
    State(state): State<AppState>,
    Query(request): Query<Connect>,
) -> Result<Json<Option<aow_zed::ConnectionStatus>>, HttpError> {
    state
        .zed
        .connection_status(&request.agent_id, request.cwd)
        .await
        .map(Json)
        .map_err(error)
}
async fn connect(
    State(state): State<AppState>,
    Json(request): Json<Connect>,
) -> Result<Json<aow_zed::ConnectionInfo>, HttpError> {
    state
        .zed
        .connect(&request.agent_id, request.cwd)
        .await
        .map(Json)
        .map_err(error)
}
async fn disconnect(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    state.zed.disconnect(&id).map_err(error)?;
    Ok(Json(json!({})))
}
#[derive(Deserialize)]
struct Auth {
    method_id: String,
}
async fn authenticate(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Auth>,
) -> Result<Json<Value>, HttpError> {
    state
        .zed
        .authenticate(&id, &request.method_id)
        .await
        .map_err(error)?;
    Ok(Json(json!({})))
}
#[derive(Default, Deserialize)]
struct SessionQuery {
    cwd: Option<String>,
    cursor: Option<String>,
}
async fn remote_sessions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Value>, HttpError> {
    state
        .zed
        .remote_sessions(&id, query.cursor)
        .await
        .map(Json)
        .map_err(error)
}
async fn import_sessions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(sessions): Json<Vec<aow_zed::SessionImport>>,
) -> Result<Json<Vec<aow_zed::SessionInfo>>, HttpError> {
    state
        .zed
        .import_sessions(&id, sessions)
        .map(Json)
        .map_err(error)
}
async fn new_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<aow_zed::SessionSnapshot>, HttpError> {
    state.zed.new_session(&id).await.map(Json).map_err(error)
}
#[derive(Deserialize)]
struct Load {
    remote_id: String,
}
async fn load_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Load>,
) -> Result<Json<aow_zed::SessionSnapshot>, HttpError> {
    state
        .zed
        .load_session(&id, &request.remote_id)
        .await
        .map(Json)
        .map_err(error)
}
async fn sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Json<Vec<aow_zed::SessionInfo>> {
    Json(state.zed.sessions(query.cwd.as_deref()))
}
async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<aow_zed::SessionSnapshot>, HttpError> {
    state.zed.snapshot(&id).map(Json).map_err(error)
}
async fn connection_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Value>>, HttpError> {
    state.zed.logs(&id).map(Json).map_err(error)
}
async fn session_logs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Value>>, HttpError> {
    state.zed.session_logs(&id).map(Json).map_err(error)
}
async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    state.zed.delete_session(&id).await.map_err(error)?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum SessionAction {
    Prompt { content: Vec<aow_zed::Content> },
    Cancel,
    DismissNotice { notice_id: String },
    Resume,
    Close,
    SetMode { mode_id: String },
    SetConfig { config_id: String, value: Value },
    Answer { request_id: String, response: Value },
}
async fn session_action(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(action): Json<SessionAction>,
) -> Result<Json<aow_zed::SessionSnapshot>, HttpError> {
    let result = match action {
        SessionAction::Prompt { content } => state.zed.start_prompt(&id, content),
        SessionAction::Cancel => state.zed.cancel(&id),
        SessionAction::DismissNotice { notice_id } => state.zed.dismiss_notice(&id, &notice_id),
        SessionAction::Resume => return state.zed.resume(&id).await.map(Json).map_err(error),
        SessionAction::Close => state.zed.close_session(&id).await,
        SessionAction::SetMode { mode_id } => state.zed.set_mode(&id, &mode_id).await,
        SessionAction::SetConfig { config_id, value } => {
            state.zed.set_config_option(&id, &config_id, value).await
        }
        SessionAction::Answer {
            request_id,
            response,
        } => state.zed.answer(&id, &request_id, response),
    };
    result.map_err(error)?;
    state.zed.snapshot(&id).map(Json).map_err(error)
}

async fn connection_requests(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<aow_zed::Permission>>, HttpError> {
    state.zed.connection_requests(&id).map(Json).map_err(error)
}
#[derive(Deserialize)]
struct AnswerRequest {
    request_id: String,
    response: Value,
}
async fn answer_connection(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<AnswerRequest>,
) -> Result<Json<Value>, HttpError> {
    state
        .zed
        .answer_connection(&id, &request.request_id, request.response)
        .map_err(error)?;
    Ok(Json(json!({})))
}
