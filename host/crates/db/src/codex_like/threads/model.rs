/// Native thread columns. Agent identity, path trust and timestamp conversion belong to callers.
#[derive(Debug, PartialEq, Eq)]
pub struct ThreadRecord {
    pub id: String,
    pub rollout_path: String,
    pub cwd: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}
