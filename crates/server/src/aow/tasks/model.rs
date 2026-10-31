use super::*;
use aow_db::tasks::TaskRecord;

pub(super) fn storage(error: anyhow::Error) -> HttpError {
    HttpError::internal(error.to_string())
}

#[derive(Serialize)]
pub(super) struct Task {
    pub id: String,
    pub project_id: String,
    pub prompt: String,
    pub agent_id: String,
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
pub(super) struct Resource {
    pub kind: String,
    pub id: String,
}

impl From<TaskRecord> for Task {
    fn from(task: TaskRecord) -> Self {
        Self {
            id: task.id,
            project_id: task.project_id,
            prompt: task.prompt,
            agent_id: task.agent_id,
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
