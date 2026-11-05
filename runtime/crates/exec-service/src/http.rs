//! Single-execution management; upper-level scheduling stays at the caller.
use aow_exec_core::ExecutionManager;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
#[derive(Clone)]
pub struct Executions {
    manager: ExecutionManager,
    node_id: Arc<String>,
    sessions: aow_agents_core::catalog::SessionCatalog,
}
use aow_runtime_contract::execution::SubmitExecution as Submit;
#[derive(Deserialize, Default)]
struct Cursor {
    node_id: Option<String>,
    #[serde(default)]
    after: u64,
}
#[derive(Deserialize)]
struct Output {
    node_id: String,
    output: String,
    #[serde(default)]
    offset: u64,
}
type Response = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;
fn error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    let message = error.to_string();
    let busy = message.contains("capacity") || message.contains("reservation is busy");
    let status = if message.contains("retired") {
        StatusCode::GONE
    } else if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        StatusCode::NOT_FOUND
    } else if busy {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    (
        status,
        Json(
            serde_json::json!({"code":if busy{"execution_busy"}else{"execution_error"},"message":message}),
        ),
    )
}
impl Executions {
    pub fn new(
        manager: ExecutionManager,
        node_id: String,
        sessions: aow_agents_core::catalog::SessionCatalog,
    ) -> Self {
        Self {
            manager,
            node_id: Arc::new(node_id),
            sessions,
        }
    }
    pub fn router(&self) -> Router {
        Router::new()
            .route(
                "/v1/executions/{id}",
                get(snapshot).put(submit).delete(remove),
            )
            .route("/v1/executions/{id}/cancel", post(cancel))
            .route("/v1/executions/{id}/output", get(output))
            .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
            .with_state(self.clone())
    }
}
async fn submit(
    State(service): State<Executions>,
    Path(id): Path<String>,
    Json(request): Json<Submit>,
) -> Response {
    if request.node_id != *service.node_id || request.spec.id != id {
        return Err(error(anyhow::anyhow!(
            "Execution node or job identity changed"
        )));
    }
    aow_i18n::spawn_blocking(move || {
        service.manager.submit(request.spec)?;
        Ok(Json(
            serde_json::json!({"id":id,"node_id":*service.node_id}),
        ))
    })
    .await
    .map_err(|error| self::error(error.into()))?
    .map_err(error)
}
async fn snapshot(
    State(service): State<Executions>,
    Path(id): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Response {
    if cursor
        .node_id
        .as_ref()
        .is_some_and(|id| id != service.node_id.as_ref())
    {
        return Err(error(anyhow::anyhow!(
            "Execution belongs to a different node"
        )));
    }
    aow_i18n::spawn_blocking(move || {
        let mut value = service.manager.snapshot(&id, cursor.after)?;
        value.node_id = service.node_id.as_ref().clone();
        if value.terminal
            && let Ok(Some(session)) = service.manager.native_session(&id)
        {
            value.native_session = Some(serde_json::from_value(
                service.sessions.remember(&session)?,
            )?);
        }
        Ok(Json(serde_json::to_value(value)?))
    })
    .await
    .map_err(|error| self::error(error.into()))?
    .map_err(error)
}
#[derive(Deserialize)]
struct Identity {
    node_id: String,
}
async fn cancel(
    State(service): State<Executions>,
    Path(id): Path<String>,
    Json(identity): Json<Identity>,
) -> Response {
    if identity.node_id != *service.node_id {
        return Err(error(anyhow::anyhow!(
            "Execution belongs to a different node"
        )));
    }
    service
        .manager
        .cancel(&id)
        .map(|()| Json(serde_json::json!({})))
        .map_err(error)
}
async fn remove(
    State(service): State<Executions>,
    Path(id): Path<String>,
    Query(identity): Query<Identity>,
) -> Response {
    if identity.node_id != *service.node_id {
        return Err(error(anyhow::anyhow!(
            "Execution belongs to a different node"
        )));
    }
    aow_i18n::spawn_blocking(move || {
        service
            .manager
            .remove(&id)
            .map(|()| Json(serde_json::json!({})))
    })
    .await
    .map_err(|e| error(e.into()))?
    .map_err(error)
}
async fn output(
    State(service): State<Executions>,
    Path(id): Path<String>,
    Query(query): Query<Output>,
) -> Response {
    if query.node_id != *service.node_id {
        return Err(error(anyhow::anyhow!(
            "Execution belongs to a different node"
        )));
    }
    aow_i18n::spawn_blocking(move || {
        service
            .manager
            .output(&id, &query.output, query.offset)
            .map(|bytes| Json(serde_json::json!({"bytes":bytes})))
    })
    .await
    .map_err(|error| self::error(error.into()))?
    .map_err(error)
}
