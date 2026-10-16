//! Zed's pending-session ownership, adapted to a durable server-side transcript.
//! Callers own waiters; the connection owns settlement of the underlying RPC.
use super::*;
use crate::facade::SessionRecovery;
use std::sync::Weak;
use tokio::sync::watch;
use tracing::Instrument as _;

type Outcome = std::result::Result<(), Arc<anyhow::Error>>;

#[derive(Default)]
pub(super) struct Loads(Mutex<HashMap<String, Arc<Load>>>);

struct Load {
    id: String,
    thread_id: String,
    state: Mutex<LoadState>,
    changed: watch::Sender<()>,
}

#[derive(Default)]
struct LoadState {
    waiters: usize,
    abandoned: bool,
    installed: bool,
    closed: bool,
    blocked: bool,
    outcome: Option<Outcome>,
}

struct Waiter(Arc<Load>);

enum Acquisition {
    Wait(Waiter, bool),
    Drain(Arc<Load>),
    Loaded,
}

#[derive(Debug)]
struct SharedLoadError(Arc<anyhow::Error>);

impl std::fmt::Display for SharedLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl std::error::Error for SharedLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref().as_ref())
    }
}

impl Loads {
    fn acquire(&self, session_id: &str, thread_id: &str) -> Result<Acquisition> {
        let mut loads = self.0.lock().unwrap();
        if let Some(load) = loads.get(session_id) {
            let mut state = load.state.lock().unwrap();
            ensure!(
                !state.blocked,
                "Previous recovery could not be cleaned up; retry the session connection to use a separate adapter"
            );
            if state.outcome.as_ref().is_some_and(Result::is_ok) {
                return Ok(Acquisition::Loaded);
            }
            if state.outcome.is_none() {
                if state.abandoned {
                    return Ok(Acquisition::Drain(load.clone()));
                }
                state.waiters += 1;
                return Ok(Acquisition::Wait(Waiter(load.clone()), false));
            }
        }
        let load = Arc::new(Load {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: thread_id.into(),
            state: Mutex::new(LoadState {
                waiters: 1,
                ..Default::default()
            }),
            changed: watch::channel(()).0,
        });
        loads.insert(session_id.into(), load.clone());
        Ok(Acquisition::Wait(Waiter(load), true))
    }

    fn for_thread(&self, thread_id: &str) -> Option<Arc<Load>> {
        self.0
            .lock()
            .unwrap()
            .values()
            .find(|load| load.thread_id == thread_id)
            .cloned()
    }

    pub(super) fn blocks_thread(&self, thread_id: &str) -> bool {
        self.for_thread(thread_id)
            .is_some_and(|load| load.state.lock().unwrap().blocked)
    }

    pub(super) fn pending_thread(&self, thread_id: &str) -> bool {
        self.for_thread(thread_id)
            .is_some_and(|load| load.state.lock().unwrap().outcome.is_none())
    }

    pub(super) fn forget(&self, session_id: &str) {
        self.0.lock().unwrap().remove(session_id);
    }
}

impl Load {
    fn abandon(&self, reason: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.installed || state.outcome.is_some() {
            return false;
        }
        if !state.abandoned {
            tracing::info!(attempt_id = %self.id, thread_id = %self.thread_id, reason, "ACP recovery abandoned; retaining RPC until settlement");
            state.abandoned = true;
            self.changed.send_replace(());
        }
        true
    }

    async fn wait(&self, caller: bool) -> Result<()> {
        let mut changed = self.changed.subscribe();
        loop {
            {
                let state = self.state.lock().unwrap();
                if caller && state.abandoned {
                    anyhow::bail!(
                        "Session recovery cancelled; the previous load is being cleaned up"
                    );
                }
                if let Some(outcome) = &state.outcome {
                    return outcome
                        .clone()
                        .map_err(|error| SharedLoadError(error).into());
                }
            }
            changed.changed().await?;
        }
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.waiters -= 1;
        if state.waiters == 0 && !state.installed && state.outcome.is_none() {
            state.abandoned = true;
            tracing::info!(attempt_id = %self.0.id, thread_id = %self.0.thread_id, "Last ACP recovery waiter dropped");
            self.0.changed.send_replace(());
        }
    }
}

impl AgentServerStore {
    pub(super) async fn open_session(
        &self,
        connection: Arc<AcpConnection>,
        session_id: String,
        id: String,
        replay: bool,
    ) -> Result<SessionSnapshot> {
        if let Ok(previous) = self.session_connection(&id) {
            ensure!(
                previous.info.id == connection.info.id || !previous.loads.pending_thread(&id),
                "Session recovery is still pending on its previous connection; wait for it to settle before switching adapters"
            );
        }
        loop {
            match connection.loads.acquire(&session_id, &id)? {
                Acquisition::Loaded => return self.snapshot(&id),
                Acquisition::Drain(load) => {
                    // No new event recipient may be registered until the old RPC and close settle.
                    let _ = load.wait(false).await;
                }
                Acquisition::Wait(waiter, start) => {
                    if start {
                        let setup = self.attach_connection(&connection, &id).and_then(|()| {
                            self.inner.threads.lock().unwrap().update(&id, |thread| {
                                thread.recovery = Some(SessionRecovery {
                                    attempt_id: waiter.0.id.clone(),
                                    phase: "loading".into(),
                                    error: None,
                                });
                            })
                        });
                        if let Err(error) = setup {
                            waiter.0.state.lock().unwrap().outcome = Some(Err(Arc::new(error)));
                            waiter.0.changed.send_replace(());
                            waiter.0.wait(true).await?;
                        }
                        connection.debug_log.bind_session(&session_id, &id);
                        let previous = connection.context.begin_load(&session_id);
                        let owner = Arc::downgrade(&self.inner);
                        let load = waiter.0.clone();
                        let connection = connection.clone();
                        let session_id = session_id.clone();
                        tokio::spawn(async move {
                            run_load(owner, connection, session_id, load, previous, replay).await;
                        });
                    }
                    waiter.0.wait(true).await?;
                    return self.snapshot(&id);
                }
            }
        }
    }

    pub(super) fn record_recovery_error(&self, id: &str, error: &anyhow::Error) -> Result<()> {
        self.inner.threads.lock().unwrap().update(id, |thread| {
            // The settlement task owns errors while a recovery is still running.
            if thread
                .recovery
                .as_ref()
                .is_none_or(|state| state.phase == "failed")
            {
                thread.recovery = Some(SessionRecovery {
                    attempt_id: thread
                        .recovery
                        .as_ref()
                        .map(|state| state.attempt_id.clone())
                        .unwrap_or_default(),
                    phase: "failed".into(),
                    error: Some(format!("{error:#}")),
                });
                thread.auth_required = crate::facade::is_auth_required(error);
            }
        })
    }

    pub(super) fn cancel_recovery(&self, id: &str) -> Result<bool> {
        let connection = self.session_connection(id)?;
        let Some(load) = connection.loads.for_thread(id) else {
            return Ok(false);
        };
        if !load.abandon("user_cancel") {
            return Ok(false);
        }
        self.recovery_phase(&load, "cleaning", None)?;
        Ok(true)
    }

    pub(super) async fn close_pending_load(&self, id: &str) -> Result<bool> {
        let Ok(connection) = self.session_connection(id) else {
            return Ok(false);
        };
        let Some(load) = connection.loads.for_thread(id) else {
            return Ok(false);
        };
        if load.state.lock().unwrap().closed {
            return Ok(true);
        }
        if !load.abandon("session_close") {
            // Installation may have won while defaults are still being applied.
            // Closing must not let these writes overtake the next session.
            let _ = load.wait(false).await;
            return Ok(false);
        }
        self.recovery_phase(&load, "cleaning", None)?;
        let _ = load.wait(false).await;
        ensure!(
            !load.state.lock().unwrap().blocked,
            "Previous recovery could not be cleaned up; retry the session connection to use a separate adapter"
        );
        Ok(load.state.lock().unwrap().closed)
    }

    fn recovery_phase(&self, load: &Load, phase: &str, error: Option<String>) -> Result<()> {
        self.inner
            .threads
            .lock()
            .unwrap()
            .update(&load.thread_id, |thread| {
                if let Some(recovery) = &mut thread.recovery
                    && recovery.attempt_id == load.id
                {
                    recovery.phase = phase.into();
                    recovery.error = error;
                }
            })
    }
}

async fn run_load(
    owner: Weak<ServiceInner>,
    connection: Arc<AcpConnection>,
    session_id: String,
    load: Arc<Load>,
    previous: Option<String>,
    replay: bool,
) {
    let started = std::time::Instant::now();
    let method = if replay {
        "session/load"
    } else {
        "session/resume"
    };
    // Unlike the SDK request future, this watcher may finish when a UI/HTTP waiter
    // disappears. The RPC stays alive until its response or connection shutdown.
    let mut changed = load.changed.subscribe();
    let request = connection.thread_request(&load.thread_id, method,
        json!({"sessionId":session_id,"cwd":connection.info.cwd,"mcpServers":[]}),
    ).instrument(tracing::info_span!("acp_recovery", attempt_id = %load.id, connection_id = %connection.info.id, session_id = %session_id, thread_id = %load.thread_id));
    tokio::pin!(request);
    let response = loop {
        // A waiter can disappear before this task first subscribes.
        if load.state.lock().unwrap().abandoned
            && let Some(inner) = owner.upgrade()
        {
            let _ = (AgentServerStore { inner }).recovery_phase(&load, "cleaning", None);
        }
        tokio::select! {
            response = &mut request => break response,
            _ = changed.changed() => {}
        }
    };
    let Some(inner) = owner.upgrade() else {
        return;
    };
    let service = AgentServerStore { inner };
    let mut cleanup = false;
    let mut outcome = {
        // Commit and cancellation have a single ordering point. No await holds this lock.
        let mut state = load.state.lock().unwrap();
        match response {
            Ok(mut result) if !state.abandoned => {
                result["sessionId"] = session_id.clone().into();
                let installed = if replay {
                    service
                        .install_session(connection.clone(), result, load.thread_id.clone())
                        .map(|_| ())
                } else {
                    install_resume(&service, &connection, &load.thread_id, result)
                };
                if installed.is_ok() {
                    state.installed = true;
                } else {
                    cleanup = true;
                }
                installed
            }
            Ok(_) => {
                cleanup = true;
                Err(anyhow::anyhow!("Session recovery cancelled"))
            }
            Err(error) => {
                // An untracked remote session may already be loaded (including by an older client).
                // Auth/cancellation errors are safe to retry on the existing connection.
                state.blocked = previous.is_none()
                    && !crate::facade::is_auth_required(&error)
                    && !error
                        .downcast_ref::<agent_client_protocol::Error>()
                        .is_some_and(|error| {
                            error.code == agent_client_protocol::ErrorCode::RequestCancelled
                        });
                Err(error)
            }
        }
    };
    if cleanup {
        let _ = service.recovery_phase(&load, "cleaning", None);
        let _ = service
            .inner
            .threads
            .lock()
            .unwrap()
            .update(&load.thread_id, |thread| {
                if thread
                    .recovery
                    .as_ref()
                    .is_some_and(|state| state.attempt_id == load.id)
                {
                    thread.needs_load = true;
                    thread.status = "disconnected".into();
                }
            });
        // Keep the old route quarantined until load settles AND close is acknowledged.
        let _ = connection.context.discard_load(&session_id, None);
        drop(service);
        let closed = if connection.supports("close") {
            connection
                .thread_request(
                    &load.thread_id,
                    "session/close",
                    json!({"sessionId":session_id}),
                )
                .await
                .map(|_| ())
        } else {
            Err(anyhow::anyhow!("Agent does not support session/close"))
        };
        if closed.is_ok() {
            load.state.lock().unwrap().closed = true;
        }
        if let Err(error) = closed {
            load.state.lock().unwrap().blocked = true;
            outcome =
                Err(error.context("Recovery cleanup failed; retry to use a separate adapter"));
        }
    } else if outcome.is_err() {
        let _ = connection
            .context
            .discard_load(&session_id, previous.as_deref());
    } else {
        // Installation has won the race. Defaults may perform further RPCs without the state lock.
        if let Err(error) = service.apply_defaults(&load.thread_id).await {
            tracing::warn!(%error, "Failed to apply recovered session defaults");
        }
    }
    if let Some(inner) = owner.upgrade() {
        let service = AgentServerStore { inner };
        if let Err(error) = &outcome {
            let _ = service.recovery_phase(&load, "failed", Some(format!("{error:#}")));
            let _ = service
                .inner
                .threads
                .lock()
                .unwrap()
                .update(&load.thread_id, |thread| {
                    if thread
                        .recovery
                        .as_ref()
                        .is_some_and(|state| state.attempt_id == load.id)
                    {
                        thread.auth_required = crate::facade::is_auth_required(error);
                    }
                });
        }
    }
    tracing::info!(attempt_id = %load.id, connection_id = %connection.info.id, session_id, elapsed_ms = started.elapsed().as_millis(), success = outcome.is_ok(), "ACP recovery settled");
    load.state.lock().unwrap().outcome = Some(outcome.map_err(Arc::new));
    load.changed.send_replace(());
}

fn install_resume(
    service: &AgentServerStore,
    connection: &AcpConnection,
    id: &str,
    result: Value,
) -> Result<()> {
    let mut thread = service.snapshot(id)?;
    thread.config_options = normalize_config_options(
        result
            .get("configOptions")
            .filter(|value| !value.is_null())
            .cloned()
            .unwrap_or(json!([])),
    )?;
    thread.config_options_supported = result
        .get("configOptions")
        .is_some_and(|value| !value.is_null());
    thread.modes = result.get("modes").cloned().unwrap_or(json!({}));
    thread.status = "idle".into();
    thread.needs_load = false;
    thread.recovery = None;
    thread.auth_required = false;
    thread.active_prompt = None;
    // Resume keeps the original transcript and its recorded prompt outcome.
    connection.context.install_session(thread, None)
}

impl Drop for ServiceInner {
    fn drop(&mut self) {
        // Detached recovery tasks must not keep an adapter alive after the service is gone.
        for connection in self.connections.get_mut().unwrap().values() {
            connection.close();
        }
    }
}
