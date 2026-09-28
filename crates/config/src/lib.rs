//! Machine configuration lives in a Git repository; runtime state stays local.

mod git;
mod layout;
mod paths;
mod repository;
mod selection;
mod storage;

pub use layout::REPOSITORY_DIRECTORY;
pub use paths::configuration_directory;
pub use repository::ConfigRepository;
pub use selection::{
    CONFIG_FILE, ConfigFile, ConfigSelection, RepositoryVersions, inspect_repository,
    save_selection,
};
