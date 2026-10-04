use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TaskStopNotification {
    pub(crate) agent: String,
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) cwd: String,
    pub(crate) turn_id: Option<String>,
    pub(crate) conclusion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) usage: Option<aow_agents::sessions::usage::TokenUsage>,
    pub(crate) instance_ids: Vec<String>,
    pub(crate) sources: Vec<TaskStopSource>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TaskStopSource {
    pub(crate) project_name: String,
    pub(crate) workspace_root: String,
    pub(crate) tab_id: String,
    pub(crate) tab_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tab_url: Option<String>,
}
