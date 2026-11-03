use super::*;
use aow_db::tasks::TaskRecord;

pub(super) fn storage(error: anyhow::Error) -> HttpError {
    HttpError::internal(error.to_string())
}

#[derive(Serialize)]
pub(crate) struct Task {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub id: String,
    pub project_id: String,
    pub prompt: String,
    pub status: String,
    pub agent_id: String,
    pub agent_role_id: Option<String>,
    pub is_router: bool,
    pub source_task_id: Option<String>,
    pub source_thread_id: Option<String>,
    pub workspace_intent: Option<String>,
    pub target_task_id: Option<String>,
    pub target_revision: Option<String>,
    pub workspace: String,
    pub workspace_mode: String,
    pub slug: Option<String>,
    pub initial_thread_id: Option<String>,
    pub pinned: bool,
    pub archived: bool,
    pub dependencies: Vec<String>,
    pub resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Resource {
    pub kind: String,
    pub id: String,
}

impl From<TaskRecord> for Task {
    fn from(task: TaskRecord) -> Self {
        Self {
            run_id: None,
            id: task.id,
            project_id: task.project_id,
            prompt: task.prompt,
            status: task.status,
            agent_id: task.agent_id,
            agent_role_id: task.agent_role_id,
            is_router: task.is_router,
            source_task_id: task.source_task_id,
            source_thread_id: task.source_thread_id,
            workspace_intent: task.workspace_intent,
            target_task_id: task.target_task_id,
            target_revision: task.target_revision,
            workspace: task.workspace,
            workspace_mode: task.workspace_mode,
            slug: task.slug,
            initial_thread_id: task.initial_thread_id,
            pinned: task.pinned,
            archived: task.archived,
            dependencies: task.dependencies,
            resources: task
                .resources
                .into_iter()
                .map(|resource| Resource {
                    kind: resource.kind,
                    id: resource.id,
                })
                .collect(),
        }
    }
}

pub(super) fn validate_prompt(prompt: &str) -> Result<(), HttpError> {
    if prompt.trim().is_empty() || prompt.len() > 131_072 {
        return Err(crate::instances::invalid(
            "Prompt must contain 1-131072 bytes",
        ));
    }
    Ok(())
}
