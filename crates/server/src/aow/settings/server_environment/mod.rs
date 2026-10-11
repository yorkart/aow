use std::{path::PathBuf, sync::Arc};

use axum::{Json, Router, extract::State, http::header, response::IntoResponse, routing::get};
use serde::{Deserialize, Serialize};

use crate::{AppState, HttpError};

mod storage;
#[cfg(test)]
mod tests;

const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone)]
struct Store {
    path: PathBuf,
    operation: Arc<std::sync::Mutex<()>>,
}

#[derive(Serialize)]
struct Document {
    path: PathBuf,
    content: String,
    revision: String,
    exists: bool,
    platform: &'static str,
}

#[derive(Deserialize)]
struct SaveRequest {
    content: String,
    revision: String,
}

pub(in crate::aow) fn routes() -> Router<AppState> {
    router(crate::PROCESS_HOME.join(".config/aow/server.env"))
}

fn router<S: Clone + Send + Sync + 'static>(path: PathBuf) -> Router<S> {
    Router::new()
        .route("/api/aow/settings/server-environment", get(read).put(save))
        .with_state(Store {
            path,
            operation: Arc::default(),
        })
}

async fn respond(
    operation: impl FnOnce() -> Result<Document, HttpError> + Send + 'static,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| HttpError::internal(error.to_string()))
        .and_then(std::convert::identity)
        .map(Json);
    ([(header::CACHE_CONTROL, "no-store")], result)
}

async fn read(State(store): State<Store>) -> impl IntoResponse {
    respond(move || store.read()).await
}

async fn save(State(store): State<Store>, Json(request): Json<SaveRequest>) -> impl IntoResponse {
    respond(move || store.save(request)).await
}
