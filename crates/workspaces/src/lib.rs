//! Shared workspace configuration and preparation. Callers own cleanup.

mod config;
mod prepare;

pub use config::{WorkspaceConfig, WorkspaceMode};
pub use prepare::{GitExecutor, PreparedWorkspace, prepare, worktree_directory};

#[cfg(test)]
mod tests;
