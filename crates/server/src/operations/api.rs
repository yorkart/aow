use std::convert::Infallible;

use aow_operation_log::{Cursor, Filter, Level, Page, ReadOptions};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use serde::Deserialize;

use super::Snapshot;
use crate::{AppState, HttpError};

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/operations/active", get(active))
        .route("/api/operations/stream", get(stream))
        .route("/api/operation-logs", get(logs))
}

async fn active(State(state): State<AppState>) -> Json<Snapshot> {
    Json(state.operations.snapshot())
}

async fn stream(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.operations.inner.changes.subscribe();
    // Subscribe before the snapshot. A reconnect always starts with a complete
    // snapshot, so dropped intermediate progress does not leave stale tasks.
    let stream =
        futures_util::stream::unfold((receiver, true), |(mut receiver, initial)| async move {
            if !initial && receiver.changed().await.is_err() {
                return None;
            }
            let snapshot = receiver.borrow_and_update().clone();
            let event = Event::default()
                .event("operations")
                .json_data(snapshot)
                .expect("serializable snapshot");
            Some((Ok(event), (receiver, false)))
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Deserialize, Default)]
struct LogQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    query: Option<String>,
    operation_id: Option<String>,
    kind: Option<String>,
    source: Option<String>,
    project_id: Option<String>,
    level: Option<Level>,
}

async fn logs(
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Page>, HttpError> {
    let cursor = query
        .cursor
        .as_ref()
        .map(|cursor| {
            if cursor.len() > 512 {
                return Err(aow_operation_log::Error::InvalidCursor);
            }
            serde_json::from_str::<Cursor>(cursor)
                .map_err(|_| aow_operation_log::Error::InvalidCursor)
        })
        .transpose()
        .map_err(log_error)?;
    let options = ReadOptions {
        cursor,
        limit: query.limit.unwrap_or(50),
        filter: Filter {
            query: query.query,
            operation_id: query.operation_id,
            kind: query.kind,
            source: query.source,
            project_id: query.project_id,
            level: query.level,
        },
        ..Default::default()
    };
    let Some(reader) = state.operations.inner.reader.clone() else {
        return Ok(Json(Page::default()));
    };
    let permit = state
        .operations
        .inner
        .reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            HttpError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "log_queries_busy",
                "日志查询繁忙，请稍后重试",
                None,
            )
        })?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        reader.read(&options)
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
    .map(Json)
    .map_err(log_error)
}

fn log_error(error: aow_operation_log::Error) -> HttpError {
    use aow_operation_log::Error;
    let (status, code) = match error {
        Error::InvalidCursor | Error::InvalidOptions => {
            (StatusCode::BAD_REQUEST, "invalid_log_query")
        }
        Error::CursorExpired => (StatusCode::GONE, "log_cursor_expired"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "log_read_failed"),
    };
    HttpError::new(status, code, error.to_string(), None)
}
