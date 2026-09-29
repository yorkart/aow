use super::super::*;
use super::{COLS, ROWS, connection::AgentConnection, model::info};
use crate::operations::{Handle, Spec};
use aow_operation_log::Outcome;
use aow_protocol::{
    AgentTerminalCreate, AgentTerminalInfo, AgentTerminalPhase, AgentTerminalState,
    AgentTerminalSubmit,
};

const MAX_TASK_BYTES: usize = 128 * 1024;

pub(crate) async fn create(
    State(state): State<AppState>,
    Json(request): Json<AgentTerminalCreate>,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    create_with_task(state, request, None).await
}

pub(crate) async fn create_for_task(
    state: AppState,
    request: AgentTerminalCreate,
    task_id: String,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    create_with_task(state, request, Some(task_id)).await
}

async fn create_with_task(
    state: AppState,
    request: AgentTerminalCreate,
    task_id: Option<String>,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    tokio::spawn(async move {
        let log = state.operations.begin(Spec {
            id: Uuid::new_v4().to_string(),
            kind: "agent.create",
            source: if task_id.is_some() { "tasks" } else { "cli" },
            title: format!("创建 Agent · {}", request.agent),
            project_id: Some(request.project_id.clone()),
            resource: Some(request.cwd.clone()),
            total: None,
        });
        let result = create_inner(state, request, &log, task_id).await;
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
    task_id: Option<String>,
) -> Result<Json<AgentTerminalInfo>, HttpError> {
    log.progress("检查 Agent 配置和工作目录", None);
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
    let adapter = launch.agent_type.agent().interactive().ok_or_else(|| {
        terminal_http_error(TerminalError::Invalid(format!(
            "interactive startup is not supported for agent {}",
            request.agent
        )))
    })?;
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
    if let Some(task_id) = &task_id {
        state.tasks.update_execution(task_id, |task| {
            task.tab_id = Some(tab.id.clone());
            task.pane_id = Some(tab.panes[0].id.clone());
        })?;
    }
    state.workspace_events.terminals_changed();
    let manager = state.terminals;
    log.resource(format!("/aow/tabs/terminal/{}", tab.id));
    let pane_id = tab.panes[0].id.clone();
    let log = log.clone();
    // Own the lifecycle independently of the HTTP client. A disconnected CLI
    // must not leave a pane permanently in Starting with no initializer.
    let events = state.workspace_events;
    let result = tokio::spawn(async move {
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
    .map_err(terminal_http_error);
    events.terminals_changed();
    result
}

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
