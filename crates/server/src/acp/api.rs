use crate::{AppState, HttpError};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/acp/settings", get(settings).put(save_settings))
        .route(
            "/api/acp/connections/{connection_id}/requests",
            get(connection_requests).post(answer_connection),
        )
        .route("/api/acp/agents", get(agents))
        .route(
            "/api/acp/agents/{agent_id}/installation",
            get(installation_status).post(install_agent),
        )
        .route("/api/acp/registry", get(registry))
        .route("/api/acp/connections", post(connect))
        .route("/api/acp/connection-status", get(connection_status))
        .route(
            "/api/acp/connections/{connection_id}/authenticate",
            post(authenticate),
        )
        .route(
            "/api/acp/connections/{connection_id}/sessions",
            get(remote_sessions).post(new_session),
        )
        .route(
            "/api/acp/connections/{connection_id}/import",
            post(import_sessions),
        )
        .route(
            "/api/acp/connections/{connection_id}/logs",
            get(connection_logs),
        )
        .route("/api/acp/sessions", get(sessions).post(prepare_session))
        .route("/api/acp/sessions/{thread_id}/metadata", get(session_info))
        .route("/api/acp/sessions/{thread_id}/fork", post(fork_session))
        .route(
            "/api/acp/sessions/{thread_id}/process",
            get(session_process),
        )
        .route(
            "/api/acp/sessions/{thread_id}/updates",
            get(session_updates),
        )
        .route(
            "/api/acp/sessions/{thread_id}/connection",
            get(session_connection),
        )
        .route(
            "/api/acp/sessions/{thread_id}",
            get(snapshot).delete(delete_session),
        )
        .route(
            "/api/acp/sessions/{thread_id}/actions",
            // Base64 image content travels inside the prompt JSON, without an upload API.
            post(session_action).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route(
            "/api/acp/sessions/{thread_id}/control",
            get(super::control::attach),
        )
        .route("/api/acp/sessions/{thread_id}/logs", get(session_logs))
        .layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response
                    .headers_mut()
                    .insert(CACHE_CONTROL, "no-store".parse().unwrap());
                response
            },
        ))
}
pub(super) fn error(error: anyhow::Error) -> HttpError {
    let missing = error.to_string() == "ACP session not found"
        || error.to_string() == "ACP connection not found";
    HttpError::new(
        if missing {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        },
        if missing {
            "acp_not_found"
        } else if aow_acp::is_auth_required(&error) {
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
) -> Result<Json<crate::aow::AcpSettingsFile>, HttpError> {
    tokio::task::spawn_blocking(move || state.aow.acp_settings())
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
) -> Result<Json<crate::aow::AcpSettingsFile>, HttpError> {
    state
        .aow
        .save_acp_settings(request.content, request.revision)
        .await
        .map(Json)
        .map_err(error)
}
async fn agents(State(state): State<AppState>) -> Result<Json<Vec<aow_acp::AgentInfo>>, HttpError> {
    state.acp.agents().await.map(Json).map_err(error)
}
async fn install_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.acp.install_agent(&agent_id).await.map_err(error)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn installation_status(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> Json<Option<aow_acp::ConnectionStatus>> {
    Json(state.acp.installation_status(&agent_id))
}
#[derive(Default, Deserialize)]
struct RegistryQuery {
    #[serde(default)]
    refresh: bool,
}
async fn registry(
    State(state): State<AppState>,
    Query(query): Query<RegistryQuery>,
) -> Result<Json<Vec<aow_acp::AgentInfo>>, HttpError> {
    state
        .acp
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
) -> Result<Json<Option<aow_acp::ConnectionStatus>>, HttpError> {
    state
        .acp
        .connection_status(&request.agent_id, request.cwd)
        .await
        .map(Json)
        .map_err(error)
}
async fn connect(
    State(state): State<AppState>,
    Json(request): Json<Connect>,
) -> Result<Json<aow_acp::ConnectionInfo>, HttpError> {
    state
        .acp
        .connect(&request.agent_id, request.cwd)
        .await
        .map(Json)
        .map_err(error)
}
#[derive(Deserialize)]
struct Auth {
    method_id: String,
}
async fn authenticate(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(request): Json<Auth>,
) -> Result<Json<Value>, HttpError> {
    state
        .acp
        .authenticate(&connection_id, &request.method_id)
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
    Path(connection_id): Path<String>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Value>, HttpError> {
    state
        .acp
        .remote_sessions(&connection_id, query.cursor)
        .await
        .map(Json)
        .map_err(error)
}
async fn import_sessions(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(sessions): Json<Vec<aow_acp::SessionImport>>,
) -> Result<Json<Vec<aow_acp::SessionInfo>>, HttpError> {
    state
        .acp
        .import_sessions(&connection_id, sessions)
        .map(Json)
        .map_err(error)
}
async fn new_session(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<aow_acp::SessionSnapshot>, HttpError> {
    state
        .acp
        .new_session(&connection_id)
        .await
        .map(Json)
        .map_err(error)
}
async fn prepare_session(
    State(state): State<AppState>,
    Json(request): Json<Connect>,
) -> Result<Json<aow_acp::SessionSnapshot>, HttpError> {
    state
        .acp
        .prepare_session(&request.agent_id, request.cwd)
        .await
        .map(Json)
        .map_err(error)
}
async fn session_info(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<aow_acp::SessionInfo>, HttpError> {
    state.acp.session_info(&thread_id).map(Json).map_err(error)
}
async fn fork_session(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<ViewQuery>,
    headers: HeaderMap,
) -> Result<Json<aow_acp::SessionSnapshot>, HttpError> {
    let controller = state.acp_controls.session(&thread_id);
    let _lifecycle = controller.authorize_lifecycle(&headers, false).await?;
    crate::instances::preserve_parent(&state, Some(&aow_protocol::InstanceRef::acp(&thread_id)))?;
    let fork = state.acp.fork_session(&thread_id).await.map_err(error)?;
    state
        .instances
        .record_all(vec![crate::instances::acp(&aow_acp::SessionInfo::from(
            &fork,
        ))])?;
    view_snapshot(&state, &fork.thread_id, &query)
}
async fn session_connection(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Option<aow_acp::ConnectionInfo>>, HttpError> {
    state
        .acp
        .session_connection_info(&thread_id)
        .map(Json)
        .map_err(error)
}
async fn sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Json<Vec<aow_acp::SessionInfo>> {
    Json(state.acp.sessions(query.cwd.as_deref()))
}
async fn snapshot(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<ViewQuery>,
) -> Result<Json<aow_acp::SessionSnapshot>, HttpError> {
    view_snapshot(&state, &thread_id, &query)
}

#[derive(Default, Deserialize)]
struct ViewQuery {
    view: Option<String>,
}

fn view_snapshot(
    state: &AppState,
    thread_id: &str,
    query: &ViewQuery,
) -> Result<Json<aow_acp::SessionSnapshot>, HttpError> {
    if query.view.as_deref() == Some("overview") {
        state.acp.overview(thread_id)
    } else {
        state.acp.snapshot(thread_id)
    }
    .map(Json)
    .map_err(error)
}

#[derive(Deserialize)]
struct UpdatesQuery {
    generation: String,
    revision: u64,
    view: Option<String>,
}

async fn session_updates(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<UpdatesQuery>,
) -> Result<Json<aow_acp::SessionUpdates>, HttpError> {
    if query.view.as_deref() == Some("overview") {
        state
            .acp
            .overview_updates(&thread_id, &query.generation, query.revision)
    } else {
        state
            .acp
            .updates(&thread_id, &query.generation, query.revision)
    }
    .map(Json)
    .map_err(error)
}

#[derive(Deserialize)]
struct ProcessQuery {
    turn_id: String,
    generation: Option<String>,
    revision: Option<u64>,
}

async fn session_process(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<ProcessQuery>,
) -> Result<Json<aow_acp::SessionUpdates>, HttpError> {
    state
        .acp
        .process(
            &thread_id,
            &query.turn_id,
            query.generation.as_deref(),
            query.revision,
        )
        .map(Json)
        .map_err(error)
}
async fn connection_logs(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<Value>>, HttpError> {
    state.acp.logs(&connection_id).map(Json).map_err(error)
}
async fn session_logs(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Vec<Value>>, HttpError> {
    state.acp.session_logs(&thread_id).map(Json).map_err(error)
}
async fn delete_session(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    let controller = state.acp_controls.session(&thread_id);
    let _lifecycle = controller.authorize_lifecycle(&headers, true).await?;
    state.acp.delete_session(&thread_id).await.map_err(error)?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum SessionAction {
    Prompt { content: Vec<aow_acp::Content> },
    Steer { content: Vec<aow_acp::Content> },
    Cancel,
    DismissNotice { notice_id: String },
    ClearPlan,
    Resume,
    Close,
    SetMode { mode_id: String },
    SetConfig { config_id: String, value: Value },
    Answer { request_id: String, response: Value },
}
async fn session_action(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<ViewQuery>,
    headers: HeaderMap,
    Json(action): Json<SessionAction>,
) -> Result<Json<aow_acp::SessionSnapshot>, HttpError> {
    let controller = state.acp_controls.session(&thread_id);
    if matches!(action, SessionAction::Resume | SessionAction::Close) {
        let _restore = controller.authorize_restore(&headers).await?;
        if matches!(action, SessionAction::Resume) {
            let resumed = state.acp.resume(&thread_id).await.map_err(error)?;
            return if query.view.as_deref() == Some("overview") {
                view_snapshot(&state, &thread_id, &query)
            } else {
                Ok(Json(resumed))
            };
        }
        state.acp.close_session(&thread_id).await.map_err(error)?;
        return view_snapshot(&state, &thread_id, &query);
    }
    controller.authorize(&headers, false)?;
    let result = match action {
        SessionAction::Prompt { content } => state.acp.start_prompt(&thread_id, content),
        SessionAction::Steer { content } => state.acp.start_steering(&thread_id, content),
        SessionAction::Cancel => state.acp.cancel(&thread_id),
        SessionAction::ClearPlan => state.acp.clear_plan(&thread_id),
        SessionAction::DismissNotice { notice_id } => {
            state.acp.dismiss_notice(&thread_id, &notice_id)
        }
        SessionAction::Resume | SessionAction::Close => {
            unreachable!("lifecycle actions are handled above")
        }
        SessionAction::SetMode { mode_id } => state.acp.set_mode(&thread_id, &mode_id).await,
        SessionAction::SetConfig { config_id, value } => {
            state
                .acp
                .set_config_option(&thread_id, &config_id, value)
                .await
        }
        SessionAction::Answer {
            request_id,
            response,
        } => state.acp.answer(&thread_id, &request_id, response),
    };
    result.map_err(error)?;
    view_snapshot(&state, &thread_id, &query)
}

async fn connection_requests(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<aow_acp::Permission>>, HttpError> {
    state
        .acp
        .connection_requests(&connection_id)
        .map(Json)
        .map_err(error)
}
#[derive(Deserialize)]
struct AnswerRequest {
    request_id: String,
    response: Value,
}
async fn answer_connection(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(request): Json<AnswerRequest>,
) -> Result<Json<Value>, HttpError> {
    // This endpoint is for connection-scoped login/import forms only. Session
    // permission replies must pass through the owned session action endpoint.
    if state
        .acp
        .connection_requests(&connection_id)
        .map_err(error)?
        .iter()
        .any(|pending| {
            pending.id == request.request_id
                && (pending.request["sessionId"]
                    .as_str()
                    .is_some_and(|connection_id| !connection_id.is_empty())
                    || pending.request["scope"]["sessionId"]
                        .as_str()
                        .is_some_and(|connection_id| !connection_id.is_empty()))
        })
    {
        return Err(HttpError::new(
            StatusCode::CONFLICT,
            "acp_control_required",
            "请通过持有控制权的会话回答此请求",
            None,
        ));
    }
    state
        .acp
        .answer_connection(&connection_id, &request.request_id, request.response)
        .map_err(error)?;
    Ok(Json(json!({})))
}
