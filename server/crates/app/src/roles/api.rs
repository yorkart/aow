use super::{RoleSession, model::RoleConfig};
use crate::error::service_http_error;
use crate::{AppState, HttpError};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/agent-roles", get(list).post(create))
        .route("/api/agent-roles/test-and-create", post(test_and_create))
        .route("/api/agent-roles/identity-sources", get(identity_sources))
        .route(
            "/api/agent-roles/{id}",
            axum::routing::put(update).delete(remove),
        )
        .route("/api/agent-roles/{id}/sessions", post(launch))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<super::AgentRole>>, HttpError> {
    state
        .roles
        .agent_roles()
        .map(Json)
        .map_err(service_http_error)
}
async fn identity_sources(
    State(state): State<AppState>,
) -> Result<Json<Vec<super::model::IdentityPreset>>, HttpError> {
    state
        .roles
        .node_role_identity_sources()
        .await
        .map(Json)
        .map_err(service_http_error)
}
async fn create(
    State(state): State<AppState>,
    Json(config): Json<RoleConfig>,
) -> Result<Json<super::AgentRole>, HttpError> {
    state
        .roles
        .save_role(None, config)
        .await
        .map(Json)
        .map_err(service_http_error)
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
        .roles
        .save_role_checked(Some(request.request_id), request.config, Some(&state))
        .await
        .map(Json)
        .map_err(service_http_error)
}
async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(config): Json<RoleConfig>,
) -> Result<Json<super::AgentRole>, HttpError> {
    state
        .roles
        .save_role(Some(id), config)
        .await
        .map(Json)
        .map_err(service_http_error)
}
async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, HttpError> {
    state
        .roles
        .remove_role(&id)
        .await
        .map_err(service_http_error)?;
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
        let _operation = state.tasks.task_resource_access().await;
        let _filesystem = state.projects.filesystem_access().await;
        let cwd = state
            .projects
            .agent_worktree_path(&request.project_id, &request.cwd)
            .await
            .map_err(service_http_error)?;
        let task = state
            .tasks
            .task_scope(&headers, std::path::Path::new(&cwd))
            .await?;
        let parent = if let Some(task_id) = &task {
            let record = state
                .tasks
                .list()
                .map_err(|error| HttpError::internal(error.to_string()))?
                .into_iter()
                .find(|item| &item.id == task_id)
                .ok_or_else(|| crate::instances::invalid("Task not found"))?;
            let parent = request.parent_thread_id.or(record.initial_thread_id);
            if let Some(parent) = &parent {
                let owner = state
                    .tasks
                    .task_for_thread(&state.acp, parent)
                    .await
                    .map_err(|error| HttpError::internal(error.to_string()))?;
                if owner.is_none_or(|owner| &owner.id != task_id) {
                    return Err(crate::instances::invalid(
                        "Parent conversation must belong to this task",
                    ));
                }
            }
            parent.map(aow_runtime_contract::instance::InstanceRef::acp)
        } else {
            None
        };
        let role = state.roles.agent_role(&id).map_err(service_http_error)?;
        if task.is_some() && matches!(role.config.runtime, super::RoleRuntime::Cli { .. }) {
            return Err(crate::instances::invalid(
                "Task conversations require an ACP role",
            ));
        }
        let session = state
            .roles
            .prepare_role_session(&role, &state, &cwd, None, parent)
            .await?;
        let (kind, id) = session.resource();
        if let Err(error) = state.tasks.attach_task_resource(task.as_deref(), kind, id) {
            session.discard(&state).await;
            return Err(error);
        }
        session.start(&state, None)?;
        Ok(Json(session))
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
}
