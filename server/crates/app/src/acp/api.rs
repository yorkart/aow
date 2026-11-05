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
pub(crate) fn error(error: anyhow::Error) -> HttpError {
    let missing = error.to_string() == "ACP session not found"
        || error.to_string() == "ACP connection not found";
    let unavailable = aow_runtime_client::acp::is_unavailable(&error);
    HttpError::new(
        if unavailable {
            StatusCode::SERVICE_UNAVAILABLE
        } else if missing {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        },
        if unavailable {
            "acpd_unavailable"
        } else if missing {
            "acp_not_found"
        } else if aow_runtime_client::acp::is_auth_required(&error) {
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
) -> Result<Json<crate::settings::AcpSettingsFile>, HttpError> {
    let settings = state.acp.settings().await.map_err(error)?;
    Ok(Json(crate::settings::AcpSettingsFile {
        path: settings.path,
        content: settings.content,
        revision: settings.revision,
    }))
}
#[derive(Deserialize)]
struct SaveSettings {
    content: String,
    revision: String,
}
async fn save_settings(
    State(state): State<AppState>,
    Json(request): Json<SaveSettings>,
) -> Result<Json<crate::settings::AcpSettingsFile>, HttpError> {
    let settings = state
        .acp
        .save_settings(request.content, request.revision)
        .await
        .map_err(error)?;
    Ok(Json(crate::settings::AcpSettingsFile {
        path: settings.path,
        content: settings.content,
        revision: settings.revision,
    }))
}
async fn agents(
    State(state): State<AppState>,
) -> Result<Json<Vec<aow_runtime_client::acp::AgentInfo>>, HttpError> {
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
) -> Result<Json<Option<aow_runtime_client::acp::ConnectionStatus>>, HttpError> {
    state
        .acp
        .installation_status(&agent_id)
        .await
        .map(Json)
        .map_err(error)
}
#[derive(Default, Deserialize)]
struct RegistryQuery {
    #[serde(default)]
    refresh: bool,
}
async fn registry(
    State(state): State<AppState>,
    Query(query): Query<RegistryQuery>,
) -> Result<Json<Vec<aow_runtime_client::acp::AgentInfo>>, HttpError> {
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
) -> Result<Json<Option<aow_runtime_client::acp::ConnectionStatus>>, HttpError> {
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
) -> Result<Json<aow_runtime_client::acp::ConnectionInfo>, HttpError> {
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
    Json(sessions): Json<Vec<aow_runtime_client::acp::SessionImport>>,
) -> Result<Json<Vec<aow_runtime_client::acp::SessionInfo>>, HttpError> {
    state
        .acp
        .import_sessions(&connection_id, sessions)
        .await
        .map(Json)
        .map_err(error)
}
async fn new_session(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<aow_runtime_client::acp::SessionSnapshot>, HttpError> {
    state
        .acp
        .new_session(&connection_id)
        .await
        .map(Json)
        .map_err(error)
}
async fn prepare_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Connect>,
) -> Result<Json<aow_runtime_client::acp::SessionSnapshot>, HttpError> {
    let _operation = state.tasks.task_resource_access().await;
    let task = state.tasks.task_scope(&headers, &request.cwd).await?;
    let session = state
        .acp
        .prepare_session(&request.agent_id, request.cwd)
        .await
        .map_err(error)?;
    if let Err(error) = state
        .tasks
        .attach_task_resource(task.as_deref(), "acp", &session.thread_id)
    {
        let _ = state.acp.delete_session(&session.thread_id).await;
        return Err(error);
    }
    Ok(Json(session))
}
async fn session_info(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<aow_runtime_client::acp::SessionInfo>, HttpError> {
    state
        .acp
        .session_info(&thread_id)
        .await
        .map(Json)
        .map_err(error)
}
async fn fork_session(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<ViewQuery>,
    headers: HeaderMap,
) -> Result<Json<aow_runtime_client::acp::SessionSnapshot>, HttpError> {
    let controller = state.acp_controls.session(&thread_id);
    let _lifecycle = controller.authorize_lifecycle(&headers, false).await?;
    crate::instances::preserve_parent(
        &state,
        Some(&aow_runtime_contract::instance::InstanceRef::acp(
            &thread_id,
        )),
    )
    .await?;
    let fork = state.acp.fork_session(&thread_id).await.map_err(error)?;
    state.instances.record_all(vec![crate::instances::acp(
        &aow_runtime_client::acp::SessionInfo::from(&fork),
    )])?;
    view_snapshot(&state, &fork.thread_id, &query).await
}
async fn session_connection(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Option<aow_runtime_client::acp::ConnectionInfo>>, HttpError> {
    state
        .acp
        .session_connection_info(&thread_id)
        .await
        .map(Json)
        .map_err(error)
}
async fn sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Vec<aow_runtime_client::acp::SessionInfo>>, HttpError> {
    state
        .acp
        .sessions(query.cwd.as_deref())
        .await
        .map(Json)
        .map_err(error)
}
async fn snapshot(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<ViewQuery>,
) -> Result<Json<aow_runtime_client::acp::SessionSnapshot>, HttpError> {
    view_snapshot(&state, &thread_id, &query).await
}

#[derive(Default, Deserialize)]
struct ViewQuery {
    view: Option<String>,
}

async fn view_snapshot(
    state: &AppState,
    thread_id: &str,
    query: &ViewQuery,
) -> Result<Json<aow_runtime_client::acp::SessionSnapshot>, HttpError> {
    if query.view.as_deref() == Some("overview") {
        state.acp.overview(thread_id).await
    } else {
        state.acp.snapshot(thread_id).await
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
) -> Result<Json<aow_runtime_client::acp::SessionUpdates>, HttpError> {
    if query.view.as_deref() == Some("overview") {
        state
            .acp
            .overview_updates(&thread_id, &query.generation, query.revision)
            .await
    } else {
        state
            .acp
            .updates(&thread_id, &query.generation, query.revision)
            .await
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
) -> Result<Json<aow_runtime_client::acp::SessionUpdates>, HttpError> {
    state
        .acp
        .process(
            &thread_id,
            &query.turn_id,
            query.generation.as_deref(),
            query.revision,
        )
        .await
        .map(Json)
        .map_err(error)
}
async fn connection_logs(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<Value>>, HttpError> {
    state
        .acp
        .logs(&connection_id)
        .await
        .map(Json)
        .map_err(error)
}
async fn session_logs(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Vec<Value>>, HttpError> {
    state
        .acp
        .session_logs(&thread_id)
        .await
        .map(Json)
        .map_err(error)
}
async fn delete_session(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    let controller = state.acp_controls.session(&thread_id);
    let _lifecycle = controller.authorize_lifecycle(&headers, true).await?;
    if state
        .tasks
        .task_for_thread(&state.acp, &thread_id)
        .await
        .map_err(error)?
        .is_some_and(|task| task.is_router && task.initial_thread_id.as_deref() == Some(&thread_id))
    {
        return Err(crate::instances::invalid(
            "The project Router conversation is persistent and cannot be deleted",
        ));
    }
    state.acp.delete_session(&thread_id).await.map_err(error)?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum SessionAction {
    Prompt {
        content: Vec<aow_runtime_client::acp::Content>,
    },
    Steer {
        content: Vec<aow_runtime_client::acp::Content>,
    },
    Cancel,
    DismissNotice {
        notice_id: String,
    },
    ClearPlan,
    Resume,
    Close,
    SetMode {
        mode_id: String,
    },
    SetConfig {
        config_id: String,
        value: Value,
    },
    Answer {
        request_id: String,
        response: Value,
    },
}
async fn session_action(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    Query(query): Query<ViewQuery>,
    headers: HeaderMap,
    Json(action): Json<SessionAction>,
) -> Result<Json<aow_runtime_client::acp::SessionSnapshot>, HttpError> {
    let controller = state.acp_controls.session(&thread_id);
    if matches!(action, SessionAction::Resume | SessionAction::Close) {
        let _restore = controller.authorize_restore(&headers).await?;
        if matches!(action, SessionAction::Resume) {
            let resumed = state.acp.resume(&thread_id).await.map_err(error)?;
            return if query.view.as_deref() == Some("overview") {
                view_snapshot(&state, &thread_id, &query).await
            } else {
                Ok(Json(resumed))
            };
        }
        state.acp.close_session(&thread_id).await.map_err(error)?;
        return view_snapshot(&state, &thread_id, &query).await;
    }
    controller.authorize(&headers, false)?;
    let result = match action {
        SessionAction::Prompt { content } => {
            let content = state
                .tasks
                .acp_role_content(&state.acp, &thread_id, content)
                .await
                .map_err(error)?;
            state.acp.start_prompt(&thread_id, content).await
        }
        SessionAction::Steer { content } => {
            let content = state
                .tasks
                .acp_role_content(&state.acp, &thread_id, content)
                .await
                .map_err(error)?;
            state.acp.start_steering(&thread_id, content).await
        }
        SessionAction::Cancel => state.acp.cancel(&thread_id).await,
        SessionAction::ClearPlan => state.acp.clear_plan(&thread_id).await,
        SessionAction::DismissNotice { notice_id } => {
            state.acp.dismiss_notice(&thread_id, &notice_id).await
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
        } => state.acp.answer(&thread_id, &request_id, response).await,
    };
    result.map_err(error)?;
    view_snapshot(&state, &thread_id, &query).await
}

async fn connection_requests(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<Vec<aow_runtime_client::acp::Permission>>, HttpError> {
    state
        .acp
        .connection_requests(&connection_id)
        .await
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
        .await
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
            aow_i18n::text("server.answer_this_request_through_the_session_holding_control"),
            None,
        ));
    }
    state
        .acp
        .answer_connection(&connection_id, &request.request_id, request.response)
        .await
        .map_err(error)?;
    Ok(Json(json!({})))
}
