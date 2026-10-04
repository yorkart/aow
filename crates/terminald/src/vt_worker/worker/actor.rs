use serde_json::json;
use std::{
    future::Future,
    sync::{Arc, Weak, atomic::Ordering},
    time::Duration,
};
use tokio::{
    process::Child,
    sync::{mpsc, watch},
    time::{Instant, sleep_until, timeout},
};

use super::super::{
    command::process_command,
    config::{MIN_SNAPSHOT_PASS_SPACING, SESSION_FAILED},
    queue::{WorkerCommand, WorkerCommandKind},
    rpc::{ActorOperation, RpcError, format_rpc_error},
    session::SessionState,
    snapshot::snapshot_dirty_sessions,
    transport::RpcClient,
};

pub(in crate::vt_worker) async fn run_worker_actor(
    mut child: Child,
    mut rpc: RpcClient,
    mut receiver: mpsc::Receiver<WorkerCommand>,
    mut shutdown: watch::Receiver<bool>,
    snapshot_interval: Duration,
) {
    let mut sessions = Vec::<Weak<SessionState>>::new();
    let mut live_session_ids = std::collections::HashSet::<String>::new();
    let mut snapshot_cursor = 0;
    let mut snapshot_deadline = Instant::now() + snapshot_interval;
    let snapshot_tick = sleep_until(snapshot_deadline);
    tokio::pin!(snapshot_tick);
    let mut next_urgent_snapshot = Instant::now();
    let failure = loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() {
                    break None;
                }
            }
            status = child.wait() => {
                break Some(match status {
                    Ok(status) => format!("VT worker exited with {status}"),
                    Err(error) => format!("failed waiting for VT worker: {error}"),
                });
            }
            response = rpc.responses.recv() => {
                // Mutation errors are handled immediately by the reader. No
                // successful reply is expected unless an RPC is awaiting it.
                break Some(match response {
                    Some(Err(error)) => format_rpc_error(&error),
                    Some(Ok(_)) => "unsolicited VT worker response".to_owned(),
                    None => "VT worker response stream closed".to_owned(),
                });
            }
            // Keep the time threshold authoritative even when writes keep the
            // command queue continuously ready. The byte threshold remains an
            // earlier fast path for bursts larger than 256 KiB.
            _ = &mut snapshot_tick => {
                let operation = async {
                    let more_dirty = snapshot_dirty_sessions(
                        &mut rpc,
                        &mut sessions,
                        &live_session_ids,
                        &mut snapshot_cursor,
                    ).await?;
                    cleanup_failed_sessions(&mut rpc, &mut sessions, &mut live_session_ids).await?;
                    Ok::<_, RpcError>(more_dirty)
                };
                let result = await_actor_operation(&mut child, &mut shutdown, operation).await;
                // Do not immediately run another overdue pass after slow
                // serialization. Commands get a fresh interval in which to
                // drain before periodic work becomes eligible again.
                let completed_at = Instant::now();
                next_urgent_snapshot = completed_at + MIN_SNAPSHOT_PASS_SPACING;
                snapshot_deadline = if matches!(
                    &result,
                    ActorOperation::Complete(Ok(true))
                ) {
                    next_urgent_snapshot
                } else {
                    completed_at + snapshot_interval
                };
                snapshot_tick.as_mut().reset(snapshot_deadline);
                match result {
                    ActorOperation::Complete(Ok(_)) => {}
                    ActorOperation::Complete(Err(error)) => {
                        break Some(format_rpc_error(&error));
                    }
                    ActorOperation::Shutdown => break None,
                    ActorOperation::WorkerExited(failure) => break Some(failure),
                }
            }
            command = receiver.recv() => {
                let Some(command) = command else { break None };
                let state = command.session().clone();
                let kind = command.kind();
                if matches!(kind, WorkerCommandKind::Create) {
                    // Track before the compound create+initial-snapshot
                    // operation. If create succeeds but snapshot fails, the
                    // cleanup pass must still dispose the Node session.
                    sessions.push(Arc::downgrade(&state));
                    live_session_ids.insert(state.session_id.clone());
                }
                // An unresponsive Node RPC must not make daemon shutdown wait
                // for the normal request timeout. Cancelling the in-flight RPC
                // is safe here because the entire worker is about to be killed
                // and every session will fall back to raw replay.
                let result = match await_actor_operation(
                    &mut child,
                    &mut shutdown,
                    process_command(command, &mut rpc),
                ).await {
                    ActorOperation::Complete(result) => result,
                    ActorOperation::Shutdown => break None,
                    ActorOperation::WorkerExited(failure) => break Some(failure),
                };
                if let Ok(snapshot_urgent) = result {
                    if snapshot_urgent {
                        // Wake the shared bounded scheduler without allowing
                        // repeated large writes/resizes to serialize once per
                        // command. Repeated wakeups retain the earliest
                        // already-scheduled deadline.
                        let urgent_deadline = Instant::now().max(next_urgent_snapshot);
                        if urgent_deadline < snapshot_deadline {
                            snapshot_deadline = urgent_deadline;
                            snapshot_tick.as_mut().reset(snapshot_deadline);
                        }
                    }
                    match kind {
                        WorkerCommandKind::Create => {}
                        WorkerCommandKind::Dispose => {
                            live_session_ids.remove(&state.session_id);
                        }
                        WorkerCommandKind::Write | WorkerCommandKind::Resize => {}
                    }
                }
                if let Err(error) = result
                    && let Some(fatal) = handle_command_error(&state, &error)
                {
                    break Some(fatal);
                }
                match await_actor_operation(
                    &mut child,
                    &mut shutdown,
                    cleanup_failed_sessions(&mut rpc, &mut sessions, &mut live_session_ids),
                ).await {
                    ActorOperation::Complete(Ok(())) => {}
                    ActorOperation::Complete(Err(error)) => {
                        break Some(format_rpc_error(&error));
                    }
                    ActorOperation::Shutdown => break None,
                    ActorOperation::WorkerExited(failure) => break Some(failure),
                }
            }
        }
    };

    receiver.close();
    while let Ok(command) = receiver.try_recv() {
        command.session().fail();
    }
    for state in sessions.iter().filter_map(Weak::upgrade) {
        state.fail();
    }
    if let Some(failure) = failure {
        tracing::warn!(%failure, "VT worker stopped; terminal runtimes will use raw replay");
    }
    let _ = child.start_kill();
    let _ = timeout(Duration::from_secs(1), child.wait()).await;
}

pub(in crate::vt_worker) async fn await_actor_operation<T, F>(
    child: &mut Child,
    shutdown: &mut watch::Receiver<bool>,
    operation: F,
) -> ActorOperation<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(operation);
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow_and_update() {
                    return ActorOperation::Shutdown;
                }
            }
            status = child.wait() => {
                return ActorOperation::WorkerExited(match status {
                    Ok(status) => format!("VT worker exited with {status}"),
                    Err(error) => format!("failed waiting for VT worker: {error}"),
                });
            }
            result = &mut operation => return ActorOperation::Complete(result),
        }
    }
}

pub(in crate::vt_worker) async fn cleanup_failed_sessions(
    rpc: &mut RpcClient,
    sessions: &mut Vec<Weak<SessionState>>,
    live_session_ids: &mut std::collections::HashSet<String>,
) -> Result<(), RpcError> {
    let failed = sessions
        .iter()
        .filter_map(Weak::upgrade)
        .filter(|session| {
            session.status.load(Ordering::Acquire) == SESSION_FAILED
                && live_session_ids.contains(&session.session_id)
        })
        .collect::<Vec<_>>();
    for session in failed {
        match rpc
            .call(
                "dispose",
                json!({
                    "session_id": session.session_id,
                    "generation": session.generation,
                }),
            )
            .await
        {
            Ok(_) => {}
            Err(RpcError::Worker(message) | RpcError::Session(message)) => {
                tracing::debug!(
                    session_id = %session.session_id,
                    %message,
                    "failed VT session was already unavailable during cleanup"
                );
            }
            Err(error) => return Err(error),
        }
        live_session_ids.remove(&session.session_id);
    }
    sessions.retain(|session| {
        session
            .upgrade()
            .is_some_and(|state| live_session_ids.contains(&state.session_id))
    });
    Ok(())
}

pub(in crate::vt_worker) fn handle_command_error(
    state: &SessionState,
    error: &RpcError,
) -> Option<String> {
    state.fail();
    match error {
        RpcError::Worker(message) | RpcError::Session(message) => {
            tracing::warn!(
                session_id = %state.session_id,
                %message,
                "VT worker rejected a session operation; using raw replay"
            );
            None
        }
        RpcError::Io(_) | RpcError::Timeout | RpcError::Protocol(_) => {
            Some(format_rpc_error(error))
        }
    }
}
