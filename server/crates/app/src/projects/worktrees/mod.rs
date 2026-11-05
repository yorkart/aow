use super::*;

mod api;
mod branches;
mod cleanup;
pub(in crate::projects) mod creation;
mod model;
mod mutations;
pub(in crate::projects) mod removal;
pub(crate) use api::routes;

pub(crate) use model::{
    CreateWorktreeRequest, CreateWorktreeResponse, ProjectBranches, SetWorktreeColorRequest,
    SetWorktreeIconRequest, Worktree, WorktreeColor, WorktreeIcon, WorktreeRemovalInspection,
    WorktreeRemovalPreview, WorktreeRemovalQuery, parse_worktrees,
};
