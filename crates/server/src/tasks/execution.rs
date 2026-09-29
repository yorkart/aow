use super::store::*;
use crate::{AppState, HttpError, terminal::agent_control};
use aow_protocol::*;
use axum::{
    Json,
    extract::{Path, State},
};

pub(super) async fn validate(state: &AppState, input: &TaskConvert) -> Result<(), HttpError> {
    let project = state
        .aow
        .registered_project(&input.project_id)
        .map_err(crate::aow::aow_http_error)?;
    let launch_cwd = if input.worktree.is_some() {
        project.repo_path.as_str()
    } else {
        &input.cwd
    };
    let launch = state
        .aow
        .resolve_agent_launch(&input.agent, launch_cwd)
        .await
        .map_err(crate::aow::aow_http_error)?;
    if launch.agent_type.agent().interactive().is_none() {
        return Err(invalid("This agent does not support interactive startup"));
    }
    if !std::path::Path::new(&input.cwd).is_absolute() {
        return Err(invalid("Workspace path must be absolute"));
    }
    if let Some(worktree) = &input.worktree {
        text(&worktree.branch, 256)?;
        text(&worktree.base_ref, 256)?;
    } else {
        let cwd = state
            .aow
            .agent_worktree_path(&input.project_id, &input.cwd)
            .await
            .map_err(crate::aow::aow_http_error)?;
        state
            .aow
            .resolve_agent_launch(&input.agent, &cwd)
            .await
            .map_err(crate::aow::aow_http_error)?;
    }
    Ok(())
}

pub(super) async fn prepare(state: AppState, input: TaskConvert) {
    let result = async {
        if let Some(worktree) = &input.worktree {
            let cwd = state
                .aow
                .create_task_worktree(
                    &input.project_id,
                    &input.cwd,
                    &worktree.branch,
                    &worktree.base_ref,
                )
                .await
                .map_err(crate::aow::aow_http_error)?;
            state.tasks.update_execution(&input.id, |t| t.cwd = cwd)?;
            state.workspace_events.projects_changed();
        }
        let task = state.tasks.get(&input.id)?;
        let prompt = if input.start_now {
            Some(prompt(&state, &task)?)
        } else {
            None
        };
        let Json(info) = agent_control::create_for_task(
            state.clone(),
            AgentTerminalCreate {
                agent: task.agent.clone(),
                project_id: task.project_id.clone(),
                cwd: task.cwd.clone(),
                task: prompt,
                timeout_seconds: 120,
            },
            task.id.clone(),
        )
        .await?;
        state.tasks.update_execution(&task.id, |t| {
            t.execution = if info.state.phase == AgentTerminalPhase::Ready {
                if info.state.task_submitted {
                    TaskExecutionPhase::Submitted
                } else {
                    TaskExecutionPhase::Ready
                }
            } else {
                TaskExecutionPhase::Failed
            };
            t.error = info.state.error;
        })?;
        Ok::<_, HttpError>(())
    }
    .await;
    if let Err(error) = result {
        if let Err(save_error) = state.tasks.update_execution(&input.id, |t| {
            t.execution = TaskExecutionPhase::Failed;
            t.error = Some(error.body.message);
        }) {
            tracing::error!(
                task_id = input.id,
                ?save_error,
                "Failed to persist task startup failure"
            );
        }
    }
}

pub(super) async fn start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<TaskRevision>,
) -> Result<Json<BoardTask>, HttpError> {
    let task = state.tasks.get(&id)?;
    let prompt = prompt(&state, &task)?;
    let pane_id = task
        .pane_id
        .ok_or_else(|| conflict("Task has no terminal yet"))?;
    state.tasks.change(|board| {
        let task = board
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(missing)?;
        revision(task.revision, input.expected_revision)?;
        if task.execution != TaskExecutionPhase::Ready {
            return Err(conflict(
                "Initial task can only be submitted once. Continue in the linked terminal.",
            ));
        }
        task.execution = TaskExecutionPhase::Submitting;
        task.revision += 1;
        task.updated_at = now();
        Ok(())
    })?;
    let task = state.tasks.get(&id)?;
    tokio::spawn(async move {
        let result = agent_control::submit(
            State(state.clone()),
            Path(pane_id),
            Json(AgentTerminalSubmit { task: prompt }),
        )
        .await;
        let update = state.tasks.update_execution(&id, |t| match result {
            Ok(_) => {
                t.execution = TaskExecutionPhase::Submitted;
                t.error = None;
            }
            Err(error) => {
                t.execution = TaskExecutionPhase::Failed;
                t.error = Some(format!(
                    "{} — inspect the terminal before resubmitting",
                    error.body.message
                ));
            }
        });
        if let Err(error) = update {
            tracing::error!(
                task_id = id,
                ?error,
                "Failed to persist task submission result"
            );
        }
    });
    Ok(Json(task))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn prompt(state: &AppState, task: &BoardTask) -> Result<String, HttpError> {
    let statuses = state.tasks.snapshot()?.statuses;
    let command = if let Some(path) = state.aow.task_state_dir() {
        format!(
            "aow-cli --state-dir {}",
            shell_quote(&path.to_string_lossy())
        )
    } else {
        "aow-cli".into()
    };
    Ok(format!(
        "{}\n\n{}\n\n[AoW task context]\nTask ID: {}\nAvailable statuses (id/name): {}\nRead status definitions: {} task statuses\nRead current state: {} task get {}\nReport status: {} task set-status {} --status STATUS-ID --expected-revision REVISION --reason 'explanation'\nRead the current revision before reporting. Statuses describe progress only; do not infer approval or execute additional work from a status label. Follow the user's instructions and continue this conversation when steered.\n",
        task.title,
        task.description,
        task.id,
        serde_json::to_string(&statuses).unwrap(),
        command,
        command,
        task.id,
        command,
        task.id
    ))
}
