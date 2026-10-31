use crate::{AppState, HttpError, instances};
use aow_protocol::AcpCreate;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/acp/agents", get(agents))
        .route("/v1/acp/sessions", get(list).post(create))
        .route("/v1/acp/sessions/{thread_id}", get(snapshot))
        .route("/v1/acp/sessions/{thread_id}/submit", post(submit))
}

fn validate(task: Option<&str>, timeout: u64) -> Result<(), HttpError> {
    if !(1..=600).contains(&timeout) {
        return Err(instances::invalid("timeout_seconds must be 1-600"));
    }
    if task.is_some_and(|task| {
        task.trim().is_empty()
            || task.len() > 131_072
            || task
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    }) {
        return Err(instances::invalid(
            "task must contain 1-131072 bytes of text without control characters",
        ));
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    Json(request): Json<AcpCreate>,
) -> Result<Json<Value>, HttpError> {
    request
        .caller
        .as_ref()
        .ok_or_else(|| instances::invalid("caller session context is required"))?
        .validate()
        .map_err(instances::invalid)?;
    validate(request.task.as_deref(), request.timeout_seconds)?;
    // The operation is independent of the requesting CLI, including preparation and first prompt.
    aow_i18n::spawn(async move {
        let cwd = state.aow.agent_worktree_path(&request.project_id, &request.cwd).await.map_err(crate::aow::aow_http_error)?;
        let parent = instances::creation_parent(&state, request.parent, request.caller.as_ref(), request.no_parent)?;
        let agent_id = state.acp.select_agent_id(request.agent_id.as_deref()).await.map_err(super::api::error)?;
        let session = state.acp.prepare_session_with_options(&agent_id, cwd.into(), aow_acp::SessionCreation { source: aow_acp::SessionSource::Cli, parent }).await.map_err(super::api::error)?;
        let id = session.thread_id;
        let info = state.acp.session_info(&id).map_err(super::api::error)?;
        if let Err(error) = state.instances.record_all(vec![instances::acp(&info)]) {
            // No adapter has started yet, so deletion cannot affect remote work.
            let _ = state.acp.delete_session(&id).await;
            return Err(error);
        }
        let mut startup = match start(state.clone(), id.clone(), request.task) {
            Ok(startup) => startup,
            Err(error) => return response(&state, &id, "failed", false, Some(error.body.message)),
        };
        match tokio::time::timeout(Duration::from_secs(request.timeout_seconds), &mut startup).await {
            Ok(result) => match result.map_err(|error| HttpError::internal(error.to_string()))? {
                Ok(task_submitted) => response(&state, &id, "ready", task_submitted, None),
                Err(error) => response(&state, &id, "failed", false, Some(error.body.message)),
            },
            // The owned task continues; returning an identity makes a timeout inspectable.
            Err(_) => response(&state, &id, "starting", false, Some("ACP startup timed out; initialization may still be running. Inspect this thread before retrying.".into())),
        }
    }).await.map_err(|error| HttpError::internal(error.to_string()))?
}

fn response(
    state: &AppState,
    id: &str,
    phase: &str,
    task_submitted: bool,
    error: Option<String>,
) -> Result<Json<Value>, HttpError> {
    let info = state.acp.session_info(id).map_err(super::api::error)?;
    let mut result =
        serde_json::to_value(info).map_err(|error| HttpError::internal(error.to_string()))?;
    result["phase"] = json!(phase);
    result["task_submitted"] = json!(task_submitted);
    result["startup_error"] = json!(error);
    Ok(Json(result))
}

pub(crate) fn start(
    state: AppState,
    id: String,
    task: Option<String>,
) -> Result<tokio::task::JoinHandle<Result<bool, HttpError>>, HttpError> {
    let controller = state.acp_controls.session(&id);
    let lease = controller.claim_cli()?;
    Ok(aow_i18n::spawn(async move {
        let _lifecycle = controller.authorize_restore(&lease.headers).await?;
        state.acp.resume(&id).await.map_err(super::api::error)?;
        // A browser takeover during initialization revokes admission of the pending prompt.
        controller.authorize(&lease.headers, false)?;
        if let Some(task) = task {
            let content = vec![aow_acp::Content {
                kind: "text".into(),
                data: serde_json::Map::from_iter([("text".into(), task.into())]),
            }];
            state.acp.start_prompt(&id, content).map_err(|error| {
                let message = error.to_string();
                if message == "ACP session is busy or disconnected" {
                    HttpError::new(axum::http::StatusCode::CONFLICT, "acp_busy", message, None)
                } else {
                    super::api::error(error)
                }
            })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }))
}

#[derive(Deserialize)]
struct Submit {
    task: String,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    120
}

async fn submit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Submit>,
) -> Result<Json<Value>, HttpError> {
    validate(Some(&request.task), request.timeout_seconds)?;
    state.acp.session_info(&id).map_err(super::api::error)?;
    let mut submission = start(state.clone(), id.clone(), Some(request.task))?;
    let task_submitted = tokio::time::timeout(
        Duration::from_secs(request.timeout_seconds),
        &mut submission,
    )
    .await
    .map_err(|_| {
        HttpError::new(
            axum::http::StatusCode::CONFLICT,
            "acp_submission_unknown",
            format!(
                "Submission to {id} timed out and may still be running; inspect it before retrying"
            ),
            None,
        )
    })?
    .map_err(|error| HttpError::internal(error.to_string()))??;
    response(&state, &id, "ready", task_submitted, None)
}

async fn agents(State(state): State<AppState>) -> Result<Json<Value>, HttpError> {
    Ok(Json(
        json!({"items":state.acp.agents().await.map_err(super::api::error)?}),
    ))
}

async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"items":state.acp.sessions(None)}))
}

async fn snapshot(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<aow_acp::SessionSnapshot>, HttpError> {
    state.acp.snapshot(&id).map(Json).map_err(super::api::error)
}

#[cfg(test)]
mod tests;
