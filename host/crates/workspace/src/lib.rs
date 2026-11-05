//! Node-owned Git, worktrees and filesystem capabilities. Callers own business policy.
mod api;
mod clipboard;
pub mod git;
pub use api::router;

mod command;
mod config;
pub use command::{GitCommandResult, NodeGit, command};
mod prepare;
mod resolve;
pub use resolve::resolve_registered_worktree;

pub use config::{WorkspaceConfig, WorkspaceMode};
pub use prepare::{GitExecutor, PreparedWorkspace, prepare, worktree_directory};

#[cfg(test)]
mod tests;
