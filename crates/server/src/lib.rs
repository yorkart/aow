//! AoW HTTP server library and public entry points.

mod aow;
mod auth;
mod automations;
mod base_path;
mod error;
mod filesystem;
mod git_api;
mod im_api;
mod local_cli;
mod notifications;
mod operations;
mod pull_requests;
mod routes;
mod session_shares;
mod state;
mod terminal;
mod web;
mod workspace_events;

pub use base_path::BasePath;
pub use error::HttpError;
pub use local_cli::start_local_cli;
pub use routes::build_router;
pub use state::{AppState, initialize_aow_state};
pub use terminal::{TerminalError, default_state_dir as default_terminal_state_dir};

pub(crate) use filesystem::{
    PROCESS_HOME, create_fs_entry, delete_fs_entry, fs_path, fs_root, fs_root_redirect, list_home,
    list_path, list_root, raw_file, read_text, rename_file_path, rename_fs_entry, write_file,
};
pub(crate) use git_api::{
    git_commit_detail, git_commit_diff, git_commit_files, git_diff, git_ignored, git_log, git_pull,
    git_push, git_repositories, git_status,
};
pub(crate) use pull_requests::{my_pull_request_detail, my_pull_request_diff, my_pull_requests};
pub(crate) use web::{
    aow_root, aow_root_redirect, health, help_page, root_redirect, serve_spa_index, spa_or_asset,
};

#[cfg(test)]
mod tests;
