mod command;
mod commit;
mod diff;
mod error;
mod history;
mod repository;
mod status;
mod watch;

pub use commit::{commit_detail, commit_diff, commit_files};
pub use diff::diff;
pub use error::GitError;
pub use history::log;
pub use repository::discover_repositories;
pub use status::{ignored_paths, pull, push, status};
pub use watch::{GitChanges, GitWatcher};

#[cfg(test)]
mod tests;
