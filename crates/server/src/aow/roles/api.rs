use super::{RoleSession, model::RoleConfig};
use crate::{AppState, HttpError, aow::aow_http_error};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;

pub(in crate::aow) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/aow/agent-roles", get(list).post(create))
        .route(
            "/api/aow/agent-roles/test-and-create",
            post(test_and_create),
        )
        .route(
            "/api/aow/agent-roles/identity-sources",
            get(identity_sources),
        )
        .route(
            "/api/aow/agent-roles/{id}",
            axum::routing::put(update).delete(remove),
        )
        .route("/api/aow/agent-roles/{id}/sessions", post(launch))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<super::AgentRole>>, HttpError> {
    state.aow.agent_roles().map(Json).map_err(aow_http_error)
}
async fn identity_sources(
    State(state): State<AppState>,
) -> Json<Vec<super::model::IdentityPreset>> {
    Json(state.aow.role_identity_sources())
}
async fn create(
    State(state): State<AppState>,
    Json(config): Json<RoleConfig>,
) -> Result<Json<super::AgentRole>, HttpError> {
    state
        .aow
        .save_role(None, config)
        .await
        .map(Json)
        .map_err(aow_http_error)
}

#[derive(Deserialize)]
struct TestedCreate {
    request_id: String,
    #[serde(flatten)]
    config: RoleConfig,
}

async fn test_and_create(
    State(state): State<AppState>,
    Json(request): Json<TestedCreate>,
) -> Result<Json<super::AgentRole>, HttpError> {
    state
        .aow
        .save_role_checked(Some(request.request_id), request.config, Some(&state))
        .await
        .map(Json)
        .map_err(aow_http_error)
}
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(config): Json<RoleConfig>,
) -> Result<Json<super::AgentRole>, HttpError> {
    state
        .aow
        .save_role(Some(id), config)
        .await
        .map(Json)
        .map_err(aow_http_error)
}
async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state.aow.remove_role(&id).await.map_err(aow_http_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Launch {
    project_id: String,
    cwd: String,
    parent_thread_id: Option<String>,
}

async fn launch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<Launch>,
) -> Result<Json<RoleSession>, HttpError> {
    aow_i18n::spawn(async move {
        let _operation = state.aow.task_resource_access().await;
        let _filesystem = state.aow.filesystem_access().await;
        let cwd = state
            .aow
            .agent_worktree_path(&request.project_id, &request.cwd)
            .await
            .map_err(aow_http_error)?;
        let task = state.aow.task_scope(&headers, std::path::Path::new(&cwd))?;
        let parent = if let Some(task_id) = &task {
            let record = state
                .aow
                .task_db()
                .list()
                .map_err(|error| HttpError::internal(error.to_string()))?
                .into_iter()
                .find(|item| &item.id == task_id)
                .ok_or_else(|| crate::instances::invalid("Task not found"))?;
            let parent = request.parent_thread_id.or(record.initial_thread_id);
            if let Some(parent) = &parent {
                let owner = state
                    .aow
                    .task_for_thread(&state.acp, parent)
                    .await
                    .map_err(|error| HttpError::internal(error.to_string()))?;
                if owner.is_none_or(|owner| &owner.id != task_id) {
                    return Err(crate::instances::invalid(
                        "Parent conversation must belong to this task",
                    ));
                }
            }
            parent.map(aow_protocol::InstanceRef::acp)
        } else {
            None
        };
        let role = state.aow.agent_role(&id).map_err(aow_http_error)?;
        if task.is_some() && matches!(role.config.runtime, super::RoleRuntime::Cli { .. }) {
            return Err(crate::instances::invalid(
                "Task conversations require an ACP role",
            ));
        }
        let session = state
            .aow
            .prepare_role_session(&role, &state, &cwd, None, parent)
            .await?;
        let (kind, id) = session.resource();
        if let Err(error) = state.aow.attach_task_resource(task.as_deref(), kind, id) {
            session.discard(&state).await;
            return Err(error);
        }
        session.start(&state, None)?;
        Ok(Json(session))
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
}
