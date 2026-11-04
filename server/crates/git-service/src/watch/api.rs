use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tokio::sync::watch;

use crate::{GitError, command::absolute};

/// Monotonic versions for worktree inventory and history in each checkout.
/// New subscribers receive the latest snapshot, including changes while idle.
#[derive(Clone, Debug, Default)]
pub struct GitChanges {
    pub revision: u64,
    pub worktrees: u64,
    pub repositories: BTreeMap<PathBuf, u64>,
}

/// Watches supplied repositories until dropped, independently of subscribers.
/// Requires a running Tokio runtime. Each root includes its linked worktrees.
pub struct GitWatcher {
    roots: watch::Sender<BTreeSet<PathBuf>>,
    changes: watch::Receiver<GitChanges>,
    task: tokio::task::JoinHandle<()>,
}

impl GitWatcher {
    pub fn new(repositories: Vec<PathBuf>) -> Result<Self, GitError> {
        let roots = repositories
            .into_iter()
            .map(absolute)
            .collect::<Result<_, _>>()?;
        let (roots, receiver) = watch::channel(roots);
        let (sender, changes) = watch::channel(GitChanges::default());
        let task = tokio::spawn(super::worker::run(receiver, sender));
        Ok(Self {
            roots,
            changes,
            task,
        })
    }

    /// Replaces the watched roots and requests a metadata reconciliation.
    pub fn set_repositories(&self, repositories: Vec<PathBuf>) -> Result<(), GitError> {
        let roots = repositories
            .into_iter()
            .map(absolute)
            .collect::<Result<_, _>>()?;
        self.roots.send_replace(roots);
        Ok(())
    }

    pub fn subscribe(&self) -> watch::Receiver<GitChanges> {
        self.changes.clone()
    }
}

impl Drop for GitWatcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}
