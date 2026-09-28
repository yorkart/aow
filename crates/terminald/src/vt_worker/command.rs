use std::sync::atomic::Ordering;

use serde_json::json;

use super::{
    config::{SESSION_DISPOSED, SNAPSHOT_BYTE_INTERVAL},
    queue::WorkerCommand,
    rpc::RpcError,
    session::SessionState,
    snapshot::validate_js_offset,
    transport::RpcClient,
};

/// Process one ordered mutation and report whether the bounded snapshot
/// scheduler should be woken immediately. No command serializes terminal
/// state inline; that keeps burst writes and resize storms on the same global
/// work budget as periodic snapshots.
pub(super) async fn process_command(
    command: WorkerCommand,
    rpc: &mut RpcClient,
) -> Result<bool, RpcError> {
    match command {
        WorkerCommand::Create {
            session,
            cols,
            rows,
        } => {
            if !session.can_process() {
                return Ok(false);
            }
            rpc.call(
                "create",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                    "initial_offset": 0,
                    "cols": cols,
                    "rows": rows,
                    "scrollback": session.scrollback,
                }),
            )
            .await?;
            session.dirty_bytes.store(1, Ordering::Release);
            Ok(true)
        }
        WorkerCommand::Write { session, batch } => {
            let batch = batch
                .lock()
                .map_err(|_| RpcError::Session("write batch lock poisoned".to_owned()))?
                .take()
                .ok_or_else(|| RpcError::Session("write batch already consumed".to_owned()))?;
            if !session.can_process() {
                return Ok(false);
            }
            validate_sent_revision(&session, batch.revision)?;
            validate_js_offset(batch.start_offset, "start_offset")?;
            let byte_length = batch.data.len();
            let expected_offset = session.sent_offset.load(Ordering::Acquire);
            if batch.start_offset != expected_offset {
                return Err(RpcError::Session(
                    "non-contiguous VT write offset".to_owned(),
                ));
            }
            let end_offset = batch
                .start_offset
                .checked_add(byte_length as u64)
                .ok_or_else(|| RpcError::Session("VT write offset overflow".to_owned()))?;
            validate_js_offset(end_offset, "end_offset")?;
            rpc.send(
                "write",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                    "start_offset": batch.start_offset,
                }),
                &batch.data,
            )
            .await?;
            session.sent_offset.store(end_offset, Ordering::Release);
            let dirty = session
                .dirty_bytes
                .fetch_add(byte_length, Ordering::AcqRel)
                .saturating_add(byte_length);
            Ok(dirty >= SNAPSHOT_BYTE_INTERVAL)
        }
        WorkerCommand::Resize {
            session,
            revision,
            at_offset,
            cols,
            rows,
        } => {
            if !session.can_process() {
                return Ok(false);
            }
            let previous_revision = revision
                .checked_sub(1)
                .ok_or_else(|| RpcError::Session("resize revision underflow".to_owned()))?;
            validate_sent_revision(&session, previous_revision)?;
            validate_js_offset(at_offset, "at_offset")?;
            if at_offset != session.sent_offset.load(Ordering::Acquire) {
                return Err(RpcError::Session(
                    "resize does not match sent offset".to_owned(),
                ));
            }
            rpc.send(
                "resize",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                    "at_offset": at_offset,
                    "cols": cols,
                    "rows": rows,
                }),
                &[],
            )
            .await?;
            session.sent_cols.store(cols, Ordering::Release);
            session.sent_rows.store(rows, Ordering::Release);
            session.sent_revision.store(revision, Ordering::Release);
            // Resize invalidated the old cache synchronously. Mark the new
            // geometry dirty and wake the same bounded scheduler used for
            // every other snapshot.
            session.dirty_bytes.store(1, Ordering::Release);
            Ok(true)
        }
        WorkerCommand::Dispose { session } => {
            if !session.can_process() {
                return Ok(false);
            }
            rpc.call(
                "dispose",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                }),
            )
            .await?;
            session.status.store(SESSION_DISPOSED, Ordering::Release);
            session.clear_snapshot();
            Ok(false)
        }
    }
}

pub(super) fn validate_sent_revision(
    session: &SessionState,
    expected: u64,
) -> Result<(), RpcError> {
    let actual = session.sent_revision.load(Ordering::Acquire);
    if actual != expected {
        return Err(RpcError::Session(format!(
            "VT session worker revision {actual}, expected {expected}"
        )));
    }
    Ok(())
}
