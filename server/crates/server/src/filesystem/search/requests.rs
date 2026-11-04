use axum::http::StatusCode;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::HttpError;

struct Entry {
    cancel: CancellationToken,
    created: Instant,
    active: bool,
}

#[derive(Clone)]
pub(crate) struct SearchRequests {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    pub capacity: Arc<Semaphore>,
}

impl Default for SearchRequests {
    fn default() -> Self {
        Self {
            entries: Arc::default(),
            capacity: Arc::new(Semaphore::new(4)),
        }
    }
}

pub(super) struct RequestGuard {
    id: String,
    requests: SearchRequests,
    pub cancel: CancellationToken,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.requests
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.id);
    }
}

impl SearchRequests {
    fn entry(&self, id: &str, active: bool) -> Result<CancellationToken, HttpError> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "invalid_search_id",
                aow_i18n::text("server.invalid_search_request_id"),
                None,
            ));
        }
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries
            .retain(|_, entry| entry.active || entry.created.elapsed() < Duration::from_secs(30));
        if entries.len() >= 128 && !entries.contains_key(id) {
            return Err(HttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "search_busy",
                aow_i18n::text("server.too_many_search_requests_try_again_later"),
                None,
            ));
        }
        // Retain early cancellations so DELETE arriving before POST still stops that search.
        let entry = entries.entry(id.to_owned()).or_insert_with(|| Entry {
            cancel: CancellationToken::new(),
            created: Instant::now(),
            active: false,
        });
        if active && entry.active {
            return Err(HttpError::new(
                StatusCode::CONFLICT,
                "duplicate_search_id",
                aow_i18n::text("server.search_request_id_is_already_in_use"),
                None,
            ));
        }
        entry.active |= active;
        if !active {
            entry.cancel.cancel();
        }
        Ok(entry.cancel.clone())
    }

    pub(super) fn begin(&self, id: String) -> Result<RequestGuard, HttpError> {
        let cancel = self.entry(&id, true)?;
        Ok(RequestGuard {
            id,
            requests: self.clone(),
            cancel,
        })
    }

    pub(super) fn cancel(&self, id: String) -> Result<(), HttpError> {
        self.entry(&id, false).map(|_| ())
    }
}
