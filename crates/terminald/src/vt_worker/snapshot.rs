use std::sync::{
    Arc, Weak,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;

use serde_json::{Value, json};

use super::{
    command::validate_sent_revision,
    config::{
        MAX_CACHED_SNAPSHOT_BYTES, MAX_JS_SAFE_INTEGER, MAX_SNAPSHOTS_PER_TICK,
        MAX_TOTAL_CACHED_SNAPSHOT_BYTES, SNAPSHOT_TICK_BUDGET,
    },
    rpc::RpcError,
    session::{SessionState, VtSnapshot},
    transport::RpcClient,
};

pub(super) fn reserve_snapshot_bytes(total: &AtomicUsize, additional: usize) -> bool {
    additional == 0
        || total
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(additional)
                    .filter(|next| *next <= MAX_TOTAL_CACHED_SNAPSHOT_BYTES)
            })
            .is_ok()
}
pub(super) async fn snapshot_dirty_sessions(
    rpc: &mut RpcClient,
    sessions: &mut Vec<Weak<SessionState>>,
    live_session_ids: &std::collections::HashSet<String>,
    cursor: &mut usize,
) -> Result<bool, RpcError> {
    sessions.retain(|session| session.strong_count() != 0);
    if sessions.is_empty() {
        *cursor = 0;
        return Ok(false);
    }
    *cursor %= sessions.len();
    let scan_limit = sessions.len();
    let started = Instant::now();
    let mut scanned = 0;
    let mut snapshotted = 0;

    while scanned < scan_limit && snapshotted < MAX_SNAPSHOTS_PER_TICK {
        if snapshotted != 0 && started.elapsed() >= SNAPSHOT_TICK_BUDGET {
            break;
        }
        let index = *cursor;
        *cursor = (*cursor + 1) % sessions.len();
        scanned += 1;
        let Some(session) = sessions[index].upgrade() else {
            continue;
        };
        if !live_session_ids.contains(&session.session_id)
            || !session.is_active()
            || session.dirty_bytes.load(Ordering::Acquire) == 0
        {
            continue;
        }
        snapshotted += 1;
        let revision = session.sent_revision.load(Ordering::Acquire);
        if let Err(error) = snapshot_session(rpc, &session, revision).await {
            session.fail();
            match error {
                RpcError::Worker(message) | RpcError::Session(message) => {
                    tracing::warn!(
                        session_id = %session.session_id,
                        %message,
                        "VT worker rejected a snapshot operation; using raw replay"
                    );
                }
                other => return Err(other),
            }
        }
    }
    Ok(sessions.iter().filter_map(Weak::upgrade).any(|session| {
        live_session_ids.contains(&session.session_id)
            && session.is_active()
            && session.dirty_bytes.load(Ordering::Acquire) != 0
    }))
}
pub(super) async fn snapshot_session(
    rpc: &mut RpcClient,
    session: &Arc<SessionState>,
    revision: u64,
) -> Result<(), RpcError> {
    if !session.can_process() {
        return Ok(());
    }
    validate_sent_revision(session, revision)?;
    let expected_offset = session.sent_offset.load(Ordering::Acquire);
    let response = rpc
        .call(
            "snapshot",
            json!({
                "session_id": session.session_id,
                "generation": session.generation,
                "scrollback": session.scrollback,
            }),
        )
        .await?;
    let mut value = response.metadata;
    validate_exact_offset(&value, expected_offset)?;
    let applied_offset = expected_offset;
    let cols = result_dimension(&value, "cols")?;
    let rows = result_dimension(&value, "rows")?;
    if cols != session.sent_cols.load(Ordering::Acquire)
        || rows != session.sent_rows.load(Ordering::Acquire)
    {
        return Err(RpcError::Session(
            "snapshot geometry does not match ordered resize".to_owned(),
        ));
    }
    if response.data.len() > MAX_CACHED_SNAPSHOT_BYTES {
        return Err(RpcError::Session(format!(
            "snapshot exceeds the {MAX_CACHED_SNAPSHOT_BYTES} byte cache limit"
        )));
    }
    if result_u64(&value, "byte_length")? != response.data.len() as u64 {
        return Err(RpcError::Protocol(
            "snapshot payload length mismatch".to_owned(),
        ));
    }
    let ansi = String::from_utf8(response.data)
        .map_err(|_| RpcError::Protocol("snapshot data is not valid UTF-8".to_owned()))?;
    let lines: Vec<String> = serde_json::from_value(
        value
            .get_mut("lines")
            .map(Value::take)
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|_| RpcError::Protocol("invalid viewport lines".to_owned()))?;
    if lines.len() > usize::from(rows)
        || lines.iter().map(String::len).sum::<usize>() > MAX_CACHED_SNAPSHOT_BYTES
    {
        return Err(RpcError::Session(
            "viewport exceeds snapshot limits".to_owned(),
        ));
    }
    session.dirty_bytes.store(0, Ordering::Release);
    session.store_snapshot(
        revision,
        VtSnapshot {
            generation: session.generation.clone(),
            applied_offset,
            cols,
            rows,
            ansi,
            lines,
        },
    );
    Ok(())
}

// Keep aligned with vt-worker/src/protocol.mjs. Xterm needs at least two
// columns for wide characters. Normalize valid one-column requests before
// queueing them; invalid zero dimensions must still be rejected by the worker.
pub(super) fn validate_exact_offset(value: &Value, expected: u64) -> Result<(), RpcError> {
    validate_js_offset(expected, "expected applied_offset")?;
    let actual = result_u64(value, "applied_offset")?;
    if actual != expected {
        return Err(RpcError::Session(format!(
            "worker applied offset {actual}, expected {expected}"
        )));
    }
    Ok(())
}

pub(super) fn validate_js_offset(value: u64, name: &str) -> Result<(), RpcError> {
    if value > MAX_JS_SAFE_INTEGER {
        return Err(RpcError::Session(format!(
            "{name} exceeds the JavaScript safe integer range"
        )));
    }
    Ok(())
}

pub(super) fn result_u64(value: &Value, name: &str) -> Result<u64, RpcError> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::Protocol(format!("result omitted integer {name}")))
}

pub(super) fn result_dimension(value: &Value, name: &str) -> Result<u16, RpcError> {
    let value = result_u64(value, name)?;
    if !(1..=1_000).contains(&value) {
        return Err(RpcError::Protocol(format!(
            "result {name} must be between 1 and 1000"
        )));
    }
    Ok(value as u16)
}
