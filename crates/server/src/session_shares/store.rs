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
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{HttpError, aow};

pub(super) const SHARES_FILE: &str = "session-shares.json";
pub(super) const CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Serialize)]
pub(super) struct ShareInfo {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) created_at: String,
}

#[derive(Clone)]
pub(crate) struct SessionShares {
    path: Option<PathBuf>,
    entries: Arc<Mutex<BTreeMap<String, Arc<LiveShare>>>>,
}

pub(super) struct LiveShare {
    record: StoredShare,
    // Serialize reads of the same transcript and briefly reuse the parsed result.
    pub(super) snapshot: tokio::sync::Mutex<Option<(Instant, AgentSessionSnapshot)>>,
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

    pub(super) fn find(
        &self,
        agent: &str,
        session_id: &str,
    ) -> Result<Option<ShareInfo>, HttpError> {
        let entries = self.entries.lock().map_err(|_| lock_error())?;
        Ok(entries
            .values()
            .find(|entry| entry.record.agent == agent && entry.record.session_id == session_id)
            .map(|entry| entry.record.info()))
    }

    pub(super) fn create(&self, locator: AgentSessionLocator) -> Result<ShareInfo, HttpError> {
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

    pub(super) fn revoke(&self, id: &str) -> Result<(), HttpError> {
        let mut entries = self.entries.lock().map_err(|_| lock_error())?;
        let mut updated = entries.clone();
        updated.retain(|_, entry| entry.record.id != id);
        if updated.len() != entries.len() {
            self.persist(&updated)?;
            *entries = updated;
        }
        Ok(())
    }

    pub(super) fn lookup(&self, token: &str) -> Result<Arc<LiveShare>, HttpError> {
        self.entries
            .lock()
            .map_err(|_| lock_error())?
            .get(token)
            .cloned()
            .ok_or_else(invalid_share)
    }

    pub(super) async fn read(&self, token: &str) -> Result<AgentSessionSnapshot, HttpError> {
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

pub(super) fn lock_error() -> HttpError {
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

pub(super) async fn read_snapshot(
    locator: AgentSessionLocator,
) -> Result<AgentSessionSnapshot, HttpError> {
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
