//! Git metadata watching, independent of project registration and HTTP.
//! Events invalidate worktree/history snapshots. A metadata-only sweep also
//! recovers missed native events without periodically running `git log`.

mod api;
mod metadata;
mod repository;
mod worker;

pub use api::{GitChanges, GitWatcher};

#[cfg(test)]
mod tests;
