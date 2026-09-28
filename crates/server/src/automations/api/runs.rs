use super::*;

#[derive(Default, Deserialize)]
struct ManualRunInput {
    revision: Option<u64>,
    #[serde(default)]
    variables: BTreeMap<String, String>,
}

pub(super) async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    let _operation = manager.operation.lock().await;
    let task = manager.task(&id).map_err(error)?;
    let request: ManualRunInput = if body.is_empty() {
        ManualRunInput::default()
    } else {
        serde_json::from_slice(&body).map_err(error)?
    };
    if (task.input.kind == TaskKind::Manual && request.revision != Some(task.revision))
        || request
            .revision
            .is_some_and(|revision| revision != task.revision)
    {
        return Err(error("任务已被修改，请刷新后重新填写变量"));
    }
    let run_id = manager
        .scheduler
        .dispatch_with_variables(&manager.store, &task, RunSource::Manual, request.variables)
        .await
        .map_err(error)?;
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "run_id": run_id })),
    ))
}

#[derive(Deserialize)]
pub(super) struct RunsQuery {
    before: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RunsQuery>,
) -> Result<Json<Vec<Run>>, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    manager.store.get_task(&id).map_err(error)?;
    Ok(Json(
        manager
            .store
            .runs(
                &id,
                query.before.as_deref(),
                query.limit.unwrap_or(50).clamp(1, 500),
            )
            .map_err(error)?,
    ))
}

pub(super) async fn run_detail(
    State(state): State<AppState>,
    Path((id, run_id)): Path<(String, String)>,
) -> Result<Json<Run>, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    manager.store.get_task(&id).map_err(error)?;
    Ok(Json(
        manager
            .store
            .read_run(&id, &run_id)
            .map_err(error)?
            .ok_or_else(|| error("执行记录尚未生成"))?,
    ))
}

pub(super) async fn run_output(
    State(state): State<AppState>,
    Path((id, run_id, output)): Path<(String, String, String)>,
) -> Result<Response, Response> {
    let manager = manager(&state).map_err(IntoResponse::into_response)?;
    manager.store.get_task(&id).map_err(error)?;
    let output = match output.as_str() {
        "stdio" => RunOutput::Stdio,
        "stderr" => RunOutput::Stderr,
        _ => return Err(error("输出类型必须是 stdio 或 stderr")),
    };
    let bytes = manager
        .store
        .read_run_output(&id, &run_id, output)
        .map_err(error)?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], bytes).into_response())
}
