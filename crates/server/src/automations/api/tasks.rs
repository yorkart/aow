use super::*;

pub(super) async fn status(
    State(state): State<AppState>,
) -> Result<Json<SchedulerStatus>, Response> {
    Ok(Json(
        manager(&state)
            .map_err(IntoResponse::into_response)?
            .scheduler
            .status()
            .await,
    ))
}

#[derive(Deserialize)]
pub(super) struct ListQuery {
    project_id: Option<String>,
}

pub(super) async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<TaskView>>, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    Ok(Json(
        manager
            .store
            .tasks()
            .map_err(error)?
            .into_iter()
            .filter(|task| {
                query
                    .project_id
                    .as_deref()
                    .is_none_or(|project_id| task.input.project_id == project_id)
            })
            .map(|task| manager.view(task))
            .collect::<Result<_>>()
            .map_err(error)?,
    ))
}

pub(super) async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskView>, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    Ok(Json(
        manager
            .view(manager.task(&id).map_err(error)?)
            .map_err(error)?,
    ))
}

async fn resolve(state: &AppState, input: &TaskInput) -> Result<(String, PathBuf, AgentLaunch)> {
    input.validate()?;
    let reference = state
        .aow
        .execution_reference_path(&input.project_id, &input.workspace)
        .await?;
    let (name, repository) = state
        .aow
        .automation_project(&input.project_id, std::path::Path::new(&reference))
        .await?;
    let agent_id = input.agent.id();
    let launch = state.aow.resolve_agent_launch(agent_id, &reference).await?;
    let environment = aow_agents::automation::configuration_environment();
    let launch = AgentLaunch {
        executable: launch.executable.into(),
        args: launch.args,
        environment,
    };
    aow_automations::agent::validate_arguments(input.agent, &launch.args)?;
    Ok((name, repository, launch))
}

fn force_worktree_cleanup(input: &mut TaskInput) {
    if input.workspace.workspace_mode == WorkspaceMode::NewWorktree {
        input.cleanup_worktree = true;
    }
}

pub(super) async fn create(
    State(state): State<AppState>,
    Json(mut input): Json<TaskInput>,
) -> Result<(StatusCode, Json<TaskView>), Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    let _operation = manager.operation.lock().await;
    force_worktree_cleanup(&mut input);
    let (project_name, repository_path, launch) = resolve(&state, &input).await.map_err(error)?;
    let task = Task {
        id: manager.store.new_task_id().map_err(error)?,
        revision: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        input,
        project_name,
        repository_path,
        launch,
        scheduler_error: None,
        deleted: false,
    };
    manager
        .scheduler
        .render(&manager.store, &task)
        .map_err(error)?;
    Ok((
        StatusCode::CREATED,
        Json(manager.save(task).await.map_err(error)?),
    ))
}

#[derive(Deserialize)]
pub(super) struct UpdateInput {
    revision: u64,
    #[serde(flatten)]
    input: TaskInput,
}

pub(super) async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut request): Json<UpdateInput>,
) -> Result<Json<TaskView>, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    let _operation = manager.operation.lock().await;
    let mut task = manager.task(&id).map_err(error)?;
    if task.revision != request.revision {
        return Err(error("任务已被修改，请刷新后重试"));
    }
    force_worktree_cleanup(&mut request.input);
    let (project_name, repository_path, launch) =
        resolve(&state, &request.input).await.map_err(error)?;
    task.input = request.input;
    task.project_name = project_name;
    task.repository_path = repository_path;
    task.launch = launch;
    task.revision += 1;
    task.updated_at = Utc::now();
    manager
        .scheduler
        .render(&manager.store, &task)
        .map_err(error)?;
    Ok(Json(manager.save(task).await.map_err(error)?))
}

#[derive(Deserialize)]
pub(super) struct EnabledInput {
    enabled: bool,
}

pub(super) async fn set_enabled(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<TaskView>, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    let _operation = manager.operation.lock().await;
    let mut task = manager.task(&id).map_err(error)?;
    task.input.enabled = input.enabled;
    if task.input.kind == TaskKind::Manual {
        return Err(error("手动任务无需暂停或启用"));
    }
    task.revision += 1;
    task.updated_at = Utc::now();
    Ok(Json(manager.save(task).await.map_err(error)?))
}

pub(super) async fn sync(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskView>, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    let _operation = manager.operation.lock().await;
    Ok(Json(
        manager
            .save(manager.task(&id).map_err(error)?)
            .await
            .map_err(error)?,
    ))
}

pub(super) async fn remove(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    let _operation = manager.operation.lock().await;
    let mut task = manager.task(&id).map_err(error)?;
    task.input.enabled = false;
    task.revision += 1;
    task.updated_at = Utc::now();
    task.scheduler_error = Some("定时器正在移除".into());
    manager.store.save_task(&task).map_err(error)?;
    task.deleted = true;
    if let Err(reason) = manager.scheduler.sync(&manager.store, &task).await {
        task.deleted = false;
        task.scheduler_error = Some(reason.to_string());
        manager.store.save_task(&task).map_err(error)?;
        return Err(error(reason));
    }
    task.scheduler_error = None;
    manager.store.save_task(&task).map_err(error)?;
    // Tombstones and run files are retained; running processes own their journals.
    Ok(StatusCode::NO_CONTENT)
}
