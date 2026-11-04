/// Stored metadata, independent of ACP's session models.
/// JSON-encoded columns are interpreted by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadRecord {
    pub thread_id: String,
    pub session_id: Option<String>,
    pub agent_id: String,
    pub title: String,
    pub updated_at: String,
    pub created_at: Option<String>,
    pub interacted_at: Option<String>,
    pub folder_paths: Option<String>,
    pub archived: bool,
    pub title_override: Option<String>,
    pub parent: Option<String>,
    pub source: String,
}
