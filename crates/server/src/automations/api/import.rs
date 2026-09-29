use super::*;
use serde_json::{Map, Value};

#[derive(Deserialize)]
pub(super) struct CreateInput {
    project_id: String,
    configuration: Map<String, Value>,
}

/// Rebind portable task settings before using the shared Web creation path.
/// Neither the caller nor a saved file can supply local identity or launch state.
pub(super) async fn create(
    State(state): State<AppState>,
    Json(mut request): Json<CreateInput>,
) -> Result<(StatusCode, Json<TaskView>), Response> {
    let project = state
        .aow
        .registered_project(&request.project_id)
        .map_err(|reason| crate::aow::aow_http_error(reason).into_response())?;
    request
        .configuration
        .insert("project_id".into(), Value::String(request.project_id));
    request
        .configuration
        .insert("workspace_path".into(), Value::String(project.repo_path));
    request
        .configuration
        .insert("enabled".into(), Value::Bool(false));
    // TaskInput reads configuration only; saved IDs, revision, timestamps,
    // launch settings, deletion and observed runtime state are not imported.
    let input: TaskInput =
        serde_json::from_value(Value::Object(request.configuration)).map_err(error)?;
    super::tasks::create(State(state), Json(input)).await
}
