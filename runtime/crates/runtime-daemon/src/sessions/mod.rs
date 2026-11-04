//! Node-owned native session catalog, process identity and transcript reads.
mod catalog;
mod model;
mod pane;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, ensure};
use aow_agents::sessions::{SessionEnvironment, SessionRoots};
use aow_terminald::TerminalObserver;
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
pub(crate) use model::SessionReference;
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub(crate) struct SessionService {
    inner: Arc<Inner>,
}
struct Inner {
    node_id: String,
    catalog: Mutex<rusqlite::Connection>,
    observer: TerminalObserver,
}
#[derive(Deserialize)]
struct Scope {
    #[serde(default)]
    cwd: PathBuf,
    agent: Option<String>,
    session_id: Option<String>,
    home: Option<PathBuf>,
    #[serde(default)]
    environment: SessionEnvironment,
    reference: Option<SessionReference>,
}
type Response = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;
fn error(
    status: StatusCode,
    message: impl std::fmt::Display,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"code":"node_session_error","message":message.to_string()})),
    )
}
impl SessionService {
    pub(crate) fn open(
        root: &std::path::Path,
        node_id: String,
        observer: TerminalObserver,
    ) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                node_id,
                catalog: Mutex::new(catalog::open(root)?),
                observer,
            }),
        })
    }
    pub(crate) fn router(&self) -> Router {
        Router::new()
            .route("/v1/agent-sessions/list", post(list))
            .route("/v1/agent-sessions/find", post(find))
            .route("/v1/agent-sessions/snapshot", post(snapshot))
            .route("/v1/agent-sessions/pane", post(pane::read))
            .route(
                "/v1/agent-sessions/process-check",
                post(pane::process_check),
            )
            .route(
                "/v1/agent-sessions/resolve-worktree",
                post(pane::resolve_worktree),
            )
            .with_state(self.clone())
    }
}
fn roots(scope: &Scope) -> Result<SessionRoots> {
    ensure!(scope.cwd.is_absolute(), "cwd must be absolute on the node");
    if let Some(agent) = &scope.agent {
        ensure!(
            aow_agents::Agent::from_id(agent)
                .and_then(aow_agents::Agent::sessions)
                .is_some(),
            "Unsupported native session provider"
        );
    }
    let home = scope
        .home
        .clone()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .context("Node HOME is unavailable")?;
    ensure!(
        home.is_absolute() && home.is_dir(),
        "Session home must exist on this node"
    );
    let mut environment: SessionEnvironment = aow_agents::KNOWN_AGENTS
        .iter()
        .flat_map(|agent| agent.definition().configuration_env.iter().copied())
        .filter_map(|key| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(|value| (key.to_owned(), PathBuf::from(value)))
        })
        .collect();
    environment.extend(scope.environment.clone());
    Ok(SessionRoots::from_configuration(&home, &environment))
}
async fn list(State(service): State<SessionService>, Json(scope): Json<Scope>) -> Response {
    let roots = roots(&scope).map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    tokio::task::spawn_blocking(move || {
        let sessions =
            aow_agents::sessions::list_sessions(&scope.cwd, scope.agent.as_deref(), roots);
        let values: Result<Vec<_>> = sessions
            .iter()
            .map(|session| service.remember(session))
            .collect();
        values
            .map(|values| Json(serde_json::json!(values)))
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))
    })
    .await
    .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
}
fn find_session(scope: &Scope, roots: SessionRoots) -> Result<aow_agents::sessions::AgentSession> {
    let agent = scope.agent.as_deref().context("Missing agent")?;
    let id = scope
        .session_id
        .as_deref()
        .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .context("Invalid session ID")?;
    aow_agents::sessions::find_session(agent, id, roots)
        .context("Native session not found on this node")
}
async fn find(State(service): State<SessionService>, Json(scope): Json<Scope>) -> Response {
    if let Some(reference) = scope.reference {
        return tokio::task::spawn_blocking(move || {
            service
                .metadata(&reference)
                .map(Json)
                .map_err(|e| error(StatusCode::GONE, e))
        })
        .await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    }
    let roots = roots(&scope).map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
    tokio::task::spawn_blocking(move || {
        let session = find_session(&scope, roots).map_err(|e| error(StatusCode::NOT_FOUND, e))?;
        service
            .remember(&session)
            .map(Json)
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))
    })
    .await
    .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
}
async fn snapshot(State(service): State<SessionService>, Json(scope): Json<Scope>) -> Response {
    let roots = if scope.reference.is_none() {
        Some(roots(&scope).map_err(|e| error(StatusCode::BAD_REQUEST, e))?)
    } else {
        None
    };
    tokio::task::spawn_blocking(move || {
        let locator = match &scope.reference {
            Some(reference) => service
                .locate(reference)
                .map_err(|e| error(StatusCode::GONE, e))?,
            None => find_session(&scope, roots.unwrap())
                .map_err(|e| error(StatusCode::NOT_FOUND, e))?
                .locator(),
        };
        let snapshot = aow_agents::sessions::snapshot::read(locator)
            .map_err(|e| error(StatusCode::GONE, e))?;
        serde_json::to_value(snapshot)
            .map(Json)
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))
    })
    .await
    .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
}
