use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use aow_agents::sessions::{
    AgentSessionLocator,
    snapshot::{self, AgentSessionSnapshot},
};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppState, HttpError, aow};

pub(crate) const PUBLIC_API_PATH: &str = "/api/public/session-shares/{token}";
const SHARES_FILE: &str = "session-shares.json";
const CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct SessionShares {
    path: Option<PathBuf>,
    entries: Arc<Mutex<BTreeMap<String, Arc<LiveShare>>>>,
}

struct LiveShare {
    record: StoredShare,
    // Serialize reads of the same transcript and briefly reuse the parsed result.
    snapshot: tokio::sync::Mutex<Option<(Instant, AgentSessionSnapshot)>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredShare {
    id: String,
    token: String,
    agent: String,
    session_id: String,
    title: String,
    cwd: PathBuf,
    transcript_path: PathBuf,
    trusted_root: PathBuf,
    created_at: String,
}

#[derive(Serialize, Deserialize)]
struct SharesDocument {
    version: u32,
    shares: Vec<StoredShare>,
}

#[derive(Serialize)]
struct ShareInfo {
    id: String,
    path: String,
    created_at: String,
}

impl StoredShare {
    fn info(&self) -> ShareInfo {
        ShareInfo {
            id: self.id.clone(),
            path: format!("/share/{}", self.token),
            created_at: self.created_at.clone(),
        }
    }

    fn locator(&self) -> Result<AgentSessionLocator, HttpError> {
        let agent = aow_agents::Agent::from_id(&self.agent)
            .filter(|agent| agent.sessions().is_some())
            .ok_or_else(unavailable)?;
        Ok(AgentSessionLocator {
            agent: agent.id(),
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            cwd: self.cwd.clone(),
            transcript_path: self.transcript_path.clone(),
            trusted_root: self.trusted_root.clone(),
        })
    }
}

impl SessionShares {
    pub(crate) fn in_memory() -> Self {
        Self {
            path: None,
            entries: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub(crate) fn persistent(state_dir: &Path) -> anyhow::Result<Self> {
        let path = state_dir.join(SHARES_FILE);
        let records = match std::fs::read(&path) {
            Ok(bytes) => {
                let document: SharesDocument = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(document.version == 1, "unsupported session shares version");
                document.shares
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        let entries = records
            .into_iter()
            .map(|record| {
                (
                    record.token.clone(),
                    Arc::new(LiveShare {
                        record,
                        snapshot: tokio::sync::Mutex::new(None),
                    }),
                )
            })
            .collect();
        Ok(Self {
            path: Some(path),
            entries: Arc::new(Mutex::new(entries)),
        })
    }

    fn persist(&self, entries: &BTreeMap<String, Arc<LiveShare>>) -> Result<(), HttpError> {
        if let Some(path) = &self.path {
            aow::atomic_save_document(
                path,
                &SharesDocument {
                    version: 1,
                    shares: entries.values().map(|entry| entry.record.clone()).collect(),
                },
            )
            .map_err(|_| HttpError::internal("无法保存会话分享设置"))?;
        }
        Ok(())
    }

    fn find(&self, agent: &str, session_id: &str) -> Result<Option<ShareInfo>, HttpError> {
        let entries = self.entries.lock().map_err(|_| lock_error())?;
        Ok(entries
            .values()
            .find(|entry| entry.record.agent == agent && entry.record.session_id == session_id)
            .map(|entry| entry.record.info()))
    }

    fn create(&self, locator: AgentSessionLocator) -> Result<ShareInfo, HttpError> {
        let mut entries = self.entries.lock().map_err(|_| lock_error())?;
        if let Some(entry) = entries.values().find(|entry| {
            entry.record.agent == locator.agent && entry.record.session_id == locator.session_id
        }) {
            return Ok(entry.record.info());
        }
        let record = StoredShare {
            id: Uuid::new_v4().simple().to_string(),
            token: Uuid::new_v4().simple().to_string(),
            agent: locator.agent.to_owned(),
            session_id: locator.session_id,
            title: locator.title,
            cwd: locator.cwd,
            transcript_path: locator.transcript_path,
            trusted_root: locator.trusted_root,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let info = record.info();
        let mut updated = entries.clone();
        updated.insert(
            record.token.clone(),
            Arc::new(LiveShare {
                record,
                snapshot: tokio::sync::Mutex::new(None),
            }),
        );
        self.persist(&updated)?;
        *entries = updated;
        Ok(info)
    }

    fn revoke(&self, id: &str) -> Result<(), HttpError> {
        let mut entries = self.entries.lock().map_err(|_| lock_error())?;
        let mut updated = entries.clone();
        updated.retain(|_, entry| entry.record.id != id);
        if updated.len() != entries.len() {
            self.persist(&updated)?;
            *entries = updated;
        }
        Ok(())
    }

    fn lookup(&self, token: &str) -> Result<Arc<LiveShare>, HttpError> {
        self.entries
            .lock()
            .map_err(|_| lock_error())?
            .get(token)
            .cloned()
            .ok_or_else(invalid_share)
    }

    async fn read(&self, token: &str) -> Result<AgentSessionSnapshot, HttpError> {
        let entry = self.lookup(token)?;
        let mut cached = entry.snapshot.lock().await;
        // Cancellation may happen while waiting for another reader.
        self.lookup(token)?;
        let snapshot = if let Some((captured, snapshot)) = &*cached
            && captured.elapsed() < CACHE_TTL
        {
            snapshot.clone()
        } else {
            let locator = entry.record.locator()?;
            let snapshot = read_snapshot(locator).await?;
            *cached = Some((Instant::now(), snapshot.clone()));
            snapshot
        };
        // Never serve an in-flight parse result after its share was revoked.
        self.lookup(token)?;
        Ok(snapshot)
    }
}

fn lock_error() -> HttpError {
    HttpError::internal("会话分享设置暂不可用")
}

fn invalid_share() -> HttpError {
    HttpError::new(
        StatusCode::NOT_FOUND,
        "session_share_not_found",
        "分享链接无效或已取消",
        None,
    )
}

fn unavailable() -> HttpError {
    HttpError::new(
        StatusCode::GONE,
        "shared_session_unavailable",
        "会话记录已不可用",
        None,
    )
}

async fn read_snapshot(locator: AgentSessionLocator) -> Result<AgentSessionSnapshot, HttpError> {
    tokio::task::spawn_blocking(move || snapshot::read(locator))
        .await
        .map_err(|_| HttpError::internal("读取会话失败，请稍后重试"))?
        .map_err(|error| match error {
            snapshot::SnapshotError::NotFound | snapshot::SnapshotError::Invalid(_) => {
                unavailable()
            }
            snapshot::SnapshotError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
                unavailable()
            }
            _ => HttpError::internal("读取会话失败，请稍后重试"),
        })
}

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

#[cfg(test)]
mod tests;
