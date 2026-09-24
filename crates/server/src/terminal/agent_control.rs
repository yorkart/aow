//! CLI-created interactive terminals. Initialization and submission own an
//! exclusive lease; browsers may observe, or explicitly acquire a read lease
//! after initialization before claiming terminal input.

use super::*;
use crate::operations::{Handle, Spec};
use aow_agents::{Agent, interactive::AgentInteractive};
use aow_operation_log::Outcome;
use aow_protocol::{
    AgentTerminalCreate, AgentTerminalInfo, AgentTerminalPhase, AgentTerminalState,
    AgentTerminalSubmit, TerminalAttachClientMessage, TerminalControlState, TerminalScreen,
};

const ROWS: u16 = 48;
const COLS: u16 = 160;
const MAX_TASK_BYTES: usize = 128 * 1024;

pub(crate) async fn create(
    State(state): State<AppState>,
    Json(request): Json<AgentTerminalCreate>,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    tokio::spawn(async move {
        let log = state.operations.begin(Spec {
            id: Uuid::new_v4().to_string(),
            kind: "agent.create",
            source: "cli",
            title: format!("创建 Agent · {}", request.agent),
            project_id: Some(request.project_id.clone()),
            resource: Some(request.cwd.clone()),
            total: None,
        });
        let result = create_inner(state, request, &log).await;
        match &result {
            Ok(info) if info.state.phase == AgentTerminalPhase::Ready => {
                log.finish(
                    Outcome::Succeeded,
                    if info.state.task_submitted {
                        "Agent 已就绪，初始任务已提交"
                    } else {
                        "Agent 已就绪"
                    },
                );
            }
            Ok(info) => log.finish(
                Outcome::Failed,
                info.state
                    .error
                    .clone()
                    .unwrap_or_else(|| "Agent 初始化失败".into()),
            ),
            Err(error) => log.finish(Outcome::Failed, error.body.message.clone()),
        }
        result
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
}

async fn create_inner(
    state: AppState,
    request: AgentTerminalCreate,
    log: &Handle,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    log.progress("检查 Agent 配置和工作目录", None);
    let adapter = Agent::from_id(&request.agent)
        .and_then(Agent::interactive)
        .ok_or_else(|| {
            terminal_http_error(TerminalError::Invalid(format!(
                "interactive startup is not supported for agent {}",
                request.agent
            )))
        })?;
    if !(1..=600).contains(&request.timeout_seconds) {
        return Err(terminal_http_error(TerminalError::Invalid(
            "timeout_seconds must be 1-600".into(),
        )));
    }
    if let Some(task) = &request.task {
        validate_task(task).map_err(terminal_http_error)?;
    }
    validate_directory(&request.cwd)
        .await
        .map_err(terminal_http_error)?;
    let cwd = state
        .aow
        .agent_worktree_path(&request.project_id, &request.cwd)
        .await
        .map_err(crate::aow::aow_http_error)?;
    let launch = state
        .aow
        .resolve_agent_launch(&request.agent, &cwd)
        .await
        .map_err(crate::aow::aow_http_error)?;
    log.progress("创建 Terminal 并启动 Agent", None);
    let tab = state
        .terminals
        .create_agent_with_state(
            CreateTerminalRequest {
                name: None,
                cwd: Some(cwd.clone()),
                workspace_root: Some(cwd),
                shell: None,
                agent_id: Some(request.agent),
                resume_session_id: None,
                rows: Some(ROWS),
                cols: Some(COLS),
            },
            launch,
            Some(AgentTerminalState {
                phase: AgentTerminalPhase::Starting,
                error: None,
                task_submitted: false,
            }),
        )
        .await
        .map_err(terminal_http_error)?;
    let manager = state.terminals;
    log.resource(format!("/aow/tabs/terminal/{}", tab.id));
    let pane_id = tab.panes[0].id.clone();
    let log = log.clone();
    // Own the lifecycle independently of the HTTP client. A disconnected CLI
    // must not leave a pane permanently in Starting with no initializer.
    tokio::spawn(async move {
        manager
            .initialize_agent_lifecycle(
                &pane_id,
                adapter,
                request.task.as_deref(),
                Duration::from_secs(request.timeout_seconds),
                Some(&log),
            )
            .await?;
        manager.cli_agent(&pane_id)
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
    .map(Json)
    .map_err(terminal_http_error)
}

#[cfg(test)]
mod tests;

pub(crate) async fn get(
    State(state): State<AppState>,
    AxumPath(pane_id): AxumPath<String>,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    state
        .terminals
        .reconcile()
        .await
        .map_err(terminal_http_error)?;
    state
        .terminals
        .cli_agent(&pane_id)
        .map(Json)
        .map_err(terminal_http_error)
}

pub(crate) async fn list(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let tabs = state
        .terminals
        .list(None)
        .await
        .map_err(terminal_http_error)?;
    let items: Vec<_> = tabs
        .tabs
        .iter()
        .flat_map(|tab| tab.panes.iter().map(move |pane| (tab, pane)))
        .filter_map(|(tab, pane)| info(tab, pane))
        .collect();
    Ok(Json(serde_json::json!({"items": items})))
}

pub(crate) async fn submit(
    State(state): State<AppState>,
    AxumPath(pane_id): AxumPath<String>,
    Json(request): Json<AgentTerminalSubmit>,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    validate_task(&request.task).map_err(terminal_http_error)?;
    // This operation also survives HTTP cancellation once started. Retrying a
    // timed-out submission is not automatic: its delivery may be ambiguous.
    tokio::spawn(async move {
        let manager = state.terminals;
        let operation = manager.agent_operation(&pane_id)?;
        let _lease = operation.try_write_owned().map_err(|_| {
            TerminalError::Conflict(
                "pane is initializing, submitting, or controlled by a user".into(),
            )
        })?;
        let mut current = manager.cli_agent(&pane_id)?;
        if current.state.phase != AgentTerminalPhase::Ready
            || current.status != TerminalPaneStatus::Running
        {
            return Err(TerminalError::Conflict("agent is not ready".into()));
        }
        tokio::time::timeout(Duration::from_secs(15), async {
            let mut connection = AgentConnection::claim(&manager.inner.terminald, &pane_id).await?;
            connection.submit(&request.task).await?;
            connection.finish().await
        })
        .await
        .map_err(|_| {
            TerminalError::Conflict(
                "submission timed out; delivery is unknown, do not retry blindly".into(),
            )
        })??;
        current.state.task_submitted = true;
        manager.set_agent_state(&pane_id, current.state)?;
        manager.cli_agent(&pane_id)
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
    .map(Json)
    .map_err(terminal_http_error)
}

fn info(tab: &TerminalTab, pane: &TerminalPane) -> Option<AgentTerminalInfo> {
    Some(AgentTerminalInfo {
        pane_id: pane.id.clone(),
        tab_id: tab.id.clone(),
        cwd: pane.cwd.clone(),
        agent: pane.agent_id.clone()?,
        status: pane.status,
        state: pane.agent_terminal.clone()?,
    })
}

impl TerminalManager {
    pub(super) fn cli_agent(&self, pane_id: &str) -> Result<AgentTerminalInfo, TerminalError> {
        let state = self.lock_state()?;
        let (tab, pane) = find_pane(&state.tabs, pane_id)
            .ok_or_else(|| TerminalError::PaneNotFound(pane_id.into()))?;
        info(&state.tabs[tab], &state.tabs[tab].panes[pane])
            .ok_or_else(|| TerminalError::Invalid("pane was not created by agent create".into()))
    }

    pub(super) fn agent_operation(
        &self,
        pane_id: &str,
    ) -> Result<Arc<tokio::sync::RwLock<()>>, TerminalError> {
        let mut operations = self
            .inner
            .agent_operations
            .lock()
            .map_err(|_| TerminalError::Poisoned)?;
        operations.retain(|_, operation| operation.strong_count() > 0);
        if let Some(operation) = operations.get(pane_id).and_then(std::sync::Weak::upgrade) {
            return Ok(operation);
        }
        let operation = Arc::new(tokio::sync::RwLock::new(()));
        operations.insert(pane_id.into(), Arc::downgrade(&operation));
        Ok(operation)
    }

    fn set_agent_state(
        &self,
        pane_id: &str,
        agent: AgentTerminalState,
    ) -> Result<(), TerminalError> {
        let mut state = self.lock_state()?;
        let (tab, pane) = find_pane(&state.tabs, pane_id)
            .ok_or_else(|| TerminalError::PaneNotFound(pane_id.into()))?;
        let old = state.tabs[tab].clone();
        state.tabs[tab].panes[pane].agent_terminal = Some(agent);
        touch_tab(&mut state.tabs[tab]);
        if let Err(error) = self.persist_locked(&state) {
            state.tabs[tab] = old;
            return Err(error);
        }
        Ok(())
    }

    pub(super) async fn initialize_agent_lifecycle(
        &self,
        pane_id: &str,
        adapter: impl AgentInteractive,
        task: Option<&str>,
        timeout: Duration,
        log: Option<&Handle>,
    ) -> Result<(), TerminalError> {
        let operation = self.agent_operation(pane_id)?;
        let _lease = operation.write_owned().await;
        if let Some(log) = log {
            log.progress("等待 Agent 就绪", None);
        }
        let result =
            tokio::time::timeout(timeout, self.initialize_agent(pane_id, adapter, task, log)).await;
        let progress = match result {
            Ok(Ok(())) => AgentTerminalState {
                phase: AgentTerminalPhase::Ready,
                error: None,
                task_submitted: task.is_some(),
            },
            outcome => {
                let error = match outcome {
                    Ok(Err(error)) => error.to_string(),
                    _ => "agent startup timed out; inspect the pane read-only (startup may require login or trust confirmation)".into(),
                };
                let mut progress = self.cli_agent(pane_id)?.state;
                progress.phase = AgentTerminalPhase::Failed;
                progress.error = Some(error);
                progress
            }
        };
        self.set_agent_state(pane_id, progress)
    }

    async fn initialize_agent(
        &self,
        pane_id: &str,
        adapter: impl AgentInteractive,
        task: Option<&str>,
        log: Option<&Handle>,
    ) -> Result<(), TerminalError> {
        let mut connection = AgentConnection::claim(&self.inner.terminald, pane_id).await?;
        self.wait_screen(pane_id, &mut connection, |screen| {
            adapter.input_ready(&screen.lines)
        })
        .await?;
        if let Some(log) = log {
            log.progress("Agent 终端已就绪", None);
        }
        if let Some(task) = task {
            if let Some(log) = log {
                log.progress("提交初始任务", None);
            }
            connection.submit(task).await?;
            let mut progress = self.cli_agent(pane_id)?.state;
            progress.task_submitted = true;
            self.set_agent_state(pane_id, progress)?;
        }
        connection.finish().await
    }

    async fn wait_screen(
        &self,
        pane_id: &str,
        connection: &mut AgentConnection,
        predicate: impl Fn(&TerminalScreen) -> bool,
    ) -> Result<TerminalScreen, TerminalError> {
        let mut interval = tokio::time::interval(Duration::from_millis(150));
        loop {
            tokio::select! {
                message = connection.socket.next() => { AgentConnection::check(message)?; }
                _ = interval.tick() => {
                    if let Some(screen) = self.inner.terminald.screen(pane_id).await.map_err(map_client_error)? {
                        if predicate(&screen) { return Ok(screen); }
                    }
                    let runtime = self.inner.terminald.get(pane_id).await.map_err(map_client_error)?;
                    if runtime.is_none_or(|runtime| runtime.status != TerminalPaneStatus::Running) {
                        return Err(TerminalError::Conflict("agent exited before becoming ready".into()));
                    }
                }
            }
        }
    }
}

fn validate_task(task: &str) -> Result<(), TerminalError> {
    if task.trim().is_empty()
        || task.len() > MAX_TASK_BYTES
        || task
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(TerminalError::Invalid(
            "task must contain 1-131072 bytes of text without terminal control characters".into(),
        ));
    }
    Ok(())
}

struct AgentConnection {
    socket: TerminaldAttachStream,
}

impl AgentConnection {
    async fn claim(client: &TerminaldClient, pane_id: &str) -> Result<Self, TerminalError> {
        let socket = client
            .attach_controlled_with_resume(pane_id, None, None)
            .await
            .map_err(map_client_error)?;
        let mut connection = Self { socket };
        connection
            .send(TerminalAttachClientMessage::Claim { force: false })
            .await?;
        loop {
            match Self::check(connection.socket.next().await)? {
                Some(TerminalAttachServerMessage::Control {
                    state: TerminalControlState::Claimed,
                }) => return Ok(connection),
                Some(TerminalAttachServerMessage::Control { .. }) => {
                    return Err(TerminalError::Conflict(
                        "pane is controlled by another client".into(),
                    ));
                }
                _ => {}
            }
        }
    }

    fn check(
        message: Option<Result<tungstenite::Message, tungstenite::Error>>,
    ) -> Result<Option<TerminalAttachServerMessage>, TerminalError> {
        match message {
            Some(Ok(tungstenite::Message::Text(text))) => {
                let message = serde_json::from_str(&text)?;
                match message {
                    TerminalAttachServerMessage::Error { message, .. } => {
                        Err(TerminalError::Conflict(message))
                    }
                    TerminalAttachServerMessage::Status { status, .. }
                        if status != TerminalPaneStatus::Running =>
                    {
                        Err(TerminalError::Conflict("agent exited".into()))
                    }
                    message => Ok(Some(message)),
                }
            }
            None | Some(Ok(tungstenite::Message::Close(_))) => {
                Err(TerminalError::Conflict("terminal connection closed".into()))
            }
            Some(Err(error)) => Err(TerminalError::Daemon(error.to_string())),
            _ => Ok(None),
        }
    }

    async fn send(&mut self, message: TerminalAttachClientMessage) -> Result<(), TerminalError> {
        self.socket
            .send(tungstenite::Message::Text(
                serde_json::to_string(&message)?.into(),
            ))
            .await
            .map_err(|error| TerminalError::Daemon(error.to_string()))
    }

    async fn write(&mut self, data: String) -> Result<(), TerminalError> {
        let request_id = Uuid::new_v4().to_string();
        self.send(TerminalAttachClientMessage::Write {
            request_id: request_id.clone(),
            data,
        })
        .await?;
        loop {
            if matches!(Self::check(self.socket.next().await)?, Some(TerminalAttachServerMessage::Written { request_id: id }) if id == request_id)
            {
                return Ok(());
            }
        }
    }

    async fn submit(&mut self, task: &str) -> Result<(), TerminalError> {
        self.write(format!("\x1b[200~{task}\x1b[201~")).await?;
        // Give the TUI's paste/input event a separate iteration before Enter.
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.write("\r".into()).await
    }

    async fn finish(&mut self) -> Result<(), TerminalError> {
        self.socket
            .close(None)
            .await
            .map_err(|error| TerminalError::Daemon(error.to_string()))?;
        // terminald releases the controller before completing the close handshake.
        while let Some(message) = self.socket.next().await {
            match message {
                Ok(tungstenite::Message::Close(_)) => break,
                other => {
                    Self::check(Some(other))?;
                }
            }
        }
        Ok(())
    }
}
