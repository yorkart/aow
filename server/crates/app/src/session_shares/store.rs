use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::HttpError;
use crate::agents::sessions::SessionReference;

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
    runtime: Option<aow_host_client::HostClient>,
    entries: Arc<Mutex<BTreeMap<String, Arc<LiveShare>>>>,
}

pub(super) struct LiveShare {
    record: StoredShare,
    // Serialize reads of the same transcript and briefly reuse the parsed result.
    pub(super) snapshot: tokio::sync::Mutex<Option<(Instant, serde_json::Value)>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredShare {
    id: String,
    token: String,
    agent: String,
    session_id: String,
    title: String,
    cwd: PathBuf,
    created_at: String,
    #[serde(default)]
    reference: Option<aow_host_client::NativeSessionReference>,
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

    fn locator(&self) -> Result<SessionReference, HttpError> {
        if let Some(reference) = &self.reference {
            return Ok(SessionReference::Node(reference.clone()));
        }
        Err(unavailable())
    }
}

impl SessionShares {
    pub(crate) fn in_memory() -> Self {
        Self {
            path: None,
            runtime: None,
            entries: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub(crate) fn persistent(state_dir: &Path) -> anyhow::Result<Self> {
        let path = state_dir.join("data").join(SHARES_FILE);
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
            runtime: None,
            entries: Arc::new(Mutex::new(entries)),
        })
    }

    pub(crate) fn with_host(mut self, runtime: Option<aow_host_client::HostClient>) -> Self {
        self.runtime = runtime;
        self
    }
    pub(super) async fn read_reference(
        &self,
        reference: SessionReference,
    ) -> Result<serde_json::Value, HttpError> {
        read_snapshot(self.runtime.as_ref(), reference).await
    }
    fn persist(&self, entries: &BTreeMap<String, Arc<LiveShare>>) -> Result<(), HttpError> {
        if let Some(path) = &self.path {
            crate::settings::persistence::atomic_save_document(
                path,
                &SharesDocument {
                    version: 1,
                    shares: entries.values().map(|entry| entry.record.clone()).collect(),
                },
            )
            .map_err(|_| {
                HttpError::internal(aow_i18n::text(
                    "server.unable_to_save_session_sharing_settings",
                ))
            })?;
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

    pub(super) fn create(
        &self,
        locator: impl Into<SessionReference>,
    ) -> Result<ShareInfo, HttpError> {
        let locator = locator.into();
        let mut entries = self.entries.lock().map_err(|_| lock_error())?;
        if let Some(entry) = entries.values().find(|entry| {
            entry.record.agent == locator.agent() && entry.record.session_id == locator.session_id()
        }) {
            return Ok(entry.record.info());
        }
        let record = StoredShare {
            id: aow_id::new_id(),
            token: Uuid::new_v4().simple().to_string(),
            agent: locator.agent().to_owned(),
            session_id: locator.session_id().to_owned(),
            title: locator.title().to_owned(),
            cwd: locator.cwd().to_path_buf(),
            reference: Some(match &locator {
                SessionReference::Node(reference) => reference.clone(),
            }),
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

    pub(super) async fn read(&self, token: &str) -> Result<serde_json::Value, HttpError> {
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
            let snapshot = self.read_reference(locator).await?;
            *cached = Some((Instant::now(), snapshot.clone()));
            snapshot
        };
        // Never serve an in-flight parse result after its share was revoked.
        self.lookup(token)?;
        Ok(snapshot)
    }
}

pub(super) fn lock_error() -> HttpError {
    HttpError::internal(aow_i18n::text(
        "server.session_sharing_settings_temporarily_unavailable",
    ))
}

fn invalid_share() -> HttpError {
    HttpError::new(
        StatusCode::NOT_FOUND,
        "session_share_not_found",
        aow_i18n::text("server.share_link_invalid_or_revoked"),
        None,
    )
}

fn unavailable() -> HttpError {
    HttpError::new(
        StatusCode::GONE,
        "shared_session_unavailable",
        aow_i18n::text("server.session_record_unavailable"),
        None,
    )
}

pub(crate) async fn read_snapshot(
    runtime: Option<&aow_host_client::HostClient>,
    locator: SessionReference,
) -> Result<serde_json::Value, HttpError> {
    match (runtime, locator) {
        (Some(runtime), SessionReference::Node(reference)) => runtime.native_snapshot(&reference).await.map_err(|error| {
            if matches!(error, aow_host_client::HostError::HttpStatus{status,..} if status == StatusCode::GONE || status == StatusCode::NOT_FOUND) {
                unavailable()
            } else {
                HttpError::new(StatusCode::SERVICE_UNAVAILABLE,"host_unavailable","Native session service is unavailable",None)
            }
        }),
        _ => Err(unavailable()),
    }
}
