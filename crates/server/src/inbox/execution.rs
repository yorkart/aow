use super::{
    api::blocking,
    model::{Execute, Run},
    store::{invalid, revision, validate_id},
};
use crate::{AppState, HttpError};
use aow_protocol::AgentTerminalCreate;
use aow_workspaces::WorkspaceMode;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

pub(super) async fn execute(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Execute>,
) -> Result<(StatusCode, Json<Run>), HttpError> {
    // Once accepted by the handler, persistence and launch survive a client
    // disconnect together; never leave a durable Starting receipt unlaunched.
    tokio::spawn(execute_inner(state, id, input))
        .await
        .map_err(|error| HttpError::internal(error.to_string()))?
}

async fn execute_inner(
    state: AppState,
    id: String,
    input: Execute,
) -> Result<(StatusCode, Json<Run>), HttpError> {
    validate_id(&input.request_key)?;
    if input.workspace.workspace_mode == WorkspaceMode::Dynamic {
        return Err(invalid("仅手动任务支持动态指定工作区"));
    }
    if let Some(run) = state.inbox.existing_run(&id, &input)? {
        return Ok((StatusCode::ACCEPTED, Json(run)));
    }
    let item = state.inbox.item(&id)?;
    revision(item.revision, input.expected_revision)?;
    let project_id = item
        .project_id
        .as_deref()
        .ok_or_else(|| invalid("请先为需求绑定项目，再选择 Agent 执行。"))?;
    input
        .workspace
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    if input.append_prompt.contains('\0') || input.append_prompt.len() > 128 * 1024 {
        return Err(invalid("追加 prompt 无效或超过 128 KiB。"));
    }
    let cwd = state
        .aow
        .execution_reference_path(project_id, &input.workspace)
        .await
        .map_err(crate::aow::aow_http_error)?;
    let launch = state
        .aow
        .resolve_agent_launch(&input.agent, &cwd)
        .await
        .map_err(crate::aow::aow_http_error)?;
    if launch.agent_type.agent().interactive().is_none() {
        return Err(invalid(
            "该 Agent 暂不支持自动提交任务，请选择 Codex、Trae CLI 或 Hermes。",
        ));
    }
    let markdown = input.final_task(&item.markdown);
    if markdown.len() > 128 * 1024 {
        return Err(invalid("需求和追加 prompt 拼接后不能超过 128 KiB。"));
    }
    let execution_fingerprint = input
        .fingerprint()
        .map_err(|error| HttpError::internal(error.to_string()))?;
    let request_item = item.clone();
    let request_input = input.clone();
    let request_cwd = if input.workspace.workspace_mode == WorkspaceMode::Temporary {
        String::new()
    } else {
        cwd.clone()
    };
    let request_markdown = markdown.clone();
    let request_fingerprint = execution_fingerprint.clone();
    let (run, fresh) = blocking(&state, move |store| {
        store.begin_run(
            &request_item,
            &request_input,
            request_cwd,
            request_markdown,
            request_fingerprint,
        )
    })
    .await?;
    if fresh {
        let background = run.clone();
        let workspace_config = input.workspace;
        let item_id = item.id;
        let request_key = input.request_key;
        state.workspace_events.inbox_changed();
        tokio::spawn(async move {
            let result = async {
                let workspace = state
                    .aow
                    .prepare_workspace(
                        &background.project_id,
                        &workspace_config,
                        &format!("inbox/{item_id}-{request_key}"),
                    )
                    .await
                    .map_err(|error| crate::aow::aow_http_error(error).body.message)?;
                if workspace.created && workspace.workspace_mode == WorkspaceMode::NewWorktree {
                    state.workspace_events.projects_changed();
                }
                let run_id = background.id.clone();
                let cwd = workspace.directory.to_string_lossy().into_owned();
                let persisted_cwd = cwd.clone();
                blocking(&state, move |store| {
                    store.set_run_cwd(&run_id, persisted_cwd)
                })
                .await
                .map_err(|error| error.body.message)?;
                crate::terminal::agent_control::create_in_workspace(
                    state.clone(),
                    AgentTerminalCreate {
                        agent: background.agent.clone(),
                        project_id: background.project_id.clone(),
                        cwd,
                        task: Some(background.markdown.clone()),
                        timeout_seconds: 120,
                    },
                    workspace,
                )
                .await
                .map(|info| info.0)
                .map_err(|error| error.body.message)
            }
            .await;
            if let Err(error) = blocking(&state, move |store| {
                store.finish_run(&background.id, result)
            })
            .await
            {
                tracing::error!(message = %error.body.message, "Failed to persist Inbox execution result");
            }
            state.workspace_events.inbox_changed();
        });
    }
    Ok((StatusCode::ACCEPTED, Json(run)))
}
