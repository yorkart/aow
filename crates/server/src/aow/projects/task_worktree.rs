//! Project defaults for task-owned worktrees. Configuration editing is intentionally separate.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::aow) struct TaskWorktreePolicy {
    /// None selects the remote default branch, then the local main-worktree branch.
    pub base_ref: Option<String>,
    pub branch_template: String,
    /// Relative to the main checkout; task worktrees always remain outside repository checkouts.
    pub worktree_parent_dir: String,
}
impl Default for TaskWorktreePolicy {
    fn default() -> Self {
        Self {
            base_ref: None,
            branch_template: "task/{{task_id}}-{{slug}}".into(),
            worktree_parent_dir: "../.aow-worktrees".into(),
        }
    }
}
