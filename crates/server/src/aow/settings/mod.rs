mod configuration;
mod manager;
mod model;
mod pinned;
mod server_environment;

pub(in crate::aow) use server_environment::routes as server_environment_routes;

pub(super) use configuration::{UpdateSettingsRequest, default_notes_base, load_settings};
pub(super) use model::{AowSettings, EditorSettings};
pub(super) use pinned::{
    PinnedDirectories, PinnedWorktrees, UpdatePinnedDirectoriesRequest,
    UpdatePinnedWorktreesRequest,
};

#[cfg(test)]
pub(super) use configuration::prepare_notes_base_directory;
