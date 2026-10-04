use super::super::agent_control::connection::AgentConnection;
use super::*;
use anyhow::{Context, ensure};

#[derive(Deserialize)]
pub(in crate::terminal) struct EnableHosting {
    task_id: String,
    revision: u64,
    #[serde(default = "TerminalHosting::default_max_inputs")]
    max_inputs: u32,
}

pub(in crate::terminal) async fn enable(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
    Json(request): Json<EnableHosting>,
) -> Result<Json<TerminalTab>, HttpError> {
    if request.max_inputs == 0 {
        return Err(terminal_http_error(TerminalError::Invalid(
            "最多自动输入次数必须为正整数".into(),
        )));
    }
    tokio::spawn(async move {
        enable_inner(state, tab_id, pane_id, request)
            .await
            .map(Json)
            .map_err(|error| terminal_http_error(TerminalError::Conflict(format!("{error:#}"))))
    })
    .await
    .map_err(|error| HttpError::internal(error.to_string()))?
}

async fn enable_inner(
    state: AppState,
    tab_id: String,
    pane_id: String,
    request: EnableHosting,
) -> anyhow::Result<TerminalTab> {
    let manager = &state.terminals;
    manager.ensure_pane(&tab_id, &pane_id)?;
    let automation = state
        .automations
        .as_ref()
        .context("Automation 服务不可用")?;
    let task = automation.hosting_task(&request.task_id, request.revision)?;
    let source = source::read(manager, &pane_id).await?;
    state
        .aow
        .automation_project_for_cwd(&task.input.project_id, Path::new(&source.process.cwd))
        .await?;
    let gate = manager.hosting_gate(&pane_id)?;
    let _gate = gate.lock().await;
    ensure!(
        manager.hosting(&pane_id)?.is_none(),
        "该实例已托管，请先接管"
    );
    let tab = manager.get_snapshot(&tab_id)?;
    let pane = tab
        .panes
        .iter()
        .find(|pane| pane.id == pane_id)
        .context("实例不存在")?;
    ensure!(pane.status == TerminalPaneStatus::Running, "终端已结束");
    ensure!(
        pane.agent_terminal
            .as_ref()
            .is_none_or(|agent| agent.phase == aow_protocol::AgentTerminalPhase::Ready),
        "Agent 尚未就绪"
    );
    let hosting = TerminalHosting {
        id: aow_id::new_id(),
        task_id: task.id,
        task_revision: task.revision,
        task_name: task.input.name,
        workspace_root: tab.workspace_root,
        agent: source.locator.agent.into(),
        session_id: source.locator.session_id,
        process: source.process,
        phase: TerminalHostingPhase::Waiting,
        phase_started_at: timestamp(),
        max_inputs: request.max_inputs,
        input_count: 0,
        source_turn_id: None,
        run_id: None,
        error: None,
    };
    // Subscribe at enable time, before claiming control, so setup does not lose
    // subsequent events. No event or transcript from before this point is replayed.
    let completions = manager.subscribe_task_completions();
    manager.set_hosting(&pane_id, None, Some(hosting.clone()))?;
    let connection = tokio::time::timeout(
        Duration::from_secs(10),
        AgentConnection::claim_with_force(&manager.inner.terminald, &pane_id, true),
    )
    .await;
    match connection {
        Ok(Ok(connection)) => worker::spawn(
            state.clone(),
            pane_id.clone(),
            hosting,
            Some(connection),
            completions,
        ),
        result => {
            let mut failed = hosting;
            failed.phase = TerminalHostingPhase::Failed;
            failed.error = Some(match result {
                Ok(Err(error)) => error.to_string(),
                _ => "获取托管控制权超时，请接管后重试".into(),
            });
            manager.set_hosting(&pane_id, Some(&failed.id.clone()), Some(failed))?;
        }
    }
    state.workspace_events.terminals_changed();
    Ok(manager.get_snapshot(&tab_id)?)
}
