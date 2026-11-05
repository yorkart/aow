//! Node-owned native session catalog, process identity and transcript reads.
mod error;
mod storage;
use error::error;
pub use error::{SessionError, SessionErrorKind};
pub use pane::{process_check, read as read_pane, resolve_worktree};
mod model;
mod pane;
#[cfg(test)]
mod tests;

use crate::sessions::observer::ManagedTerminals;
use crate::sessions::{SessionEnvironment, SessionRoots};
use anyhow::{Context, Result, ensure};
use aow_runtime_contract::agents::NativeSessionQuery as Scope;
pub(crate) use model::SessionReference;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct SessionCatalog {
    inner: Arc<Inner>,
}
struct Inner {
    node_id: String,
    catalog: Mutex<rusqlite::Connection>,
    observer: Arc<dyn ManagedTerminals>,
}
type SessionResult = Result<serde_json::Value, SessionError>;
impl SessionCatalog {
    pub fn open(
        root: &std::path::Path,
        node_id: String,
        observer: Arc<dyn ManagedTerminals>,
    ) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                node_id,
                catalog: Mutex::new(storage::open(root)?),
                observer,
            }),
        })
    }
}

fn roots(scope: &Scope) -> Result<SessionRoots> {
    ensure!(scope.cwd.is_absolute(), "cwd must be absolute on the node");
    if let Some(agent) = &scope.agent {
        ensure!(
            crate::Agent::from_id(agent)
                .and_then(crate::Agent::sessions)
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
    let mut environment: SessionEnvironment = crate::KNOWN_AGENTS
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
pub async fn list(service: SessionCatalog, scope: Scope) -> SessionResult {
    let roots = roots(&scope).map_err(|e| error(SessionErrorKind::Invalid, e))?;
    aow_i18n::spawn_blocking(move || {
        let sessions = crate::sessions::list_sessions(&scope.cwd, scope.agent.as_deref(), roots);
        let values: Result<Vec<_>> = sessions
            .iter()
            .map(|session| service.remember(session))
            .collect();
        values
            .map(|values| serde_json::json!(values))
            .map_err(|e| error(SessionErrorKind::Internal, e))
    })
    .await
    .map_err(|e| error(SessionErrorKind::Internal, e))?
}
fn find_session(scope: &Scope, roots: SessionRoots) -> Result<crate::sessions::AgentSession> {
    let agent = scope.agent.as_deref().context("Missing agent")?;
    let id = scope
        .session_id
        .as_deref()
        .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .context("Invalid session ID")?;
    crate::sessions::find_session(agent, id, roots).context("Native session not found on this node")
}
pub async fn find(service: SessionCatalog, scope: Scope) -> SessionResult {
    if let Some(reference) = scope.reference {
        return aow_i18n::spawn_blocking(move || {
            service
                .metadata(&reference)
                .map_err(|e| error(SessionErrorKind::Unavailable, e))
        })
        .await
        .map_err(|e| error(SessionErrorKind::Internal, e))?;
    }
    let roots = roots(&scope).map_err(|e| error(SessionErrorKind::Invalid, e))?;
    aow_i18n::spawn_blocking(move || {
        let session =
            find_session(&scope, roots).map_err(|e| error(SessionErrorKind::NotFound, e))?;
        service
            .remember(&session)
            .map_err(|e| error(SessionErrorKind::Internal, e))
    })
    .await
    .map_err(|e| error(SessionErrorKind::Internal, e))?
}
pub async fn snapshot(service: SessionCatalog, scope: Scope) -> SessionResult {
    let roots = if scope.reference.is_none() {
        Some(roots(&scope).map_err(|e| error(SessionErrorKind::Invalid, e))?)
    } else {
        None
    };
    aow_i18n::spawn_blocking(move || {
        let locator = match &scope.reference {
            Some(reference) => service
                .locate(reference)
                .map_err(|e| error(SessionErrorKind::Unavailable, e))?,
            None => find_session(&scope, roots.unwrap())
                .map_err(|e| error(SessionErrorKind::NotFound, e))?
                .locator(),
        };
        let snapshot = crate::sessions::snapshot::read(locator)
            .map_err(|e| error(SessionErrorKind::Unavailable, e))?;
        serde_json::to_value(snapshot).map_err(|e| error(SessionErrorKind::Internal, e))
    })
    .await
    .map_err(|e| error(SessionErrorKind::Internal, e))?
}
