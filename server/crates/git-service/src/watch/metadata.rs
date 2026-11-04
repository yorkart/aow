use notify::RecursiveMode;
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

use super::repository::Repository;

type Fingerprint = BTreeMap<PathBuf, u64>;
pub(super) type Watches = BTreeMap<PathBuf, RecursiveMode>;

#[derive(Default, PartialEq, Eq, Debug)]
pub(super) struct Metadata {
    pub(super) history: Fingerprint,
    pub(super) worktrees: Fingerprint,
    pub(super) roots: BTreeSet<PathBuf>,
}

// Read contents instead of mtimes: atomic replacement and packed refs must
// invalidate even when timestamps have coarse resolution. Never read objects,
// index, reflogs or lock files (status itself can write the index).
fn record(path: PathBuf, snapshot: &mut Fingerprint) {
    let mut hash = DefaultHasher::new();
    std::fs::read(&path).ok().hash(&mut hash);
    snapshot.insert(path, hash.finish());
}

fn watch_directory(path: &Path, mode: RecursiveMode, watches: &mut Watches) {
    if path.is_dir() {
        watches.insert(path.to_owned(), mode);
    }
}

fn record_tree(path: &Path, snapshot: &mut Fingerprint) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            record_tree(&entry.path(), snapshot);
        } else if kind.is_file() && !entry.file_name().to_string_lossy().ends_with(".lock") {
            record(entry.path(), snapshot);
        }
    }
}

pub(super) fn collect(repository: &Repository, watches: &mut Watches) -> Metadata {
    let mut snapshot = Metadata::default();
    snapshot
        .roots
        .extend([repository.root.clone(), repository.main.clone()]);
    // Read common refs once alongside the per-worktree metadata directories.
    let mut directories = BTreeSet::from([repository.common.clone(), repository.git.clone()]);
    let worktrees = repository.common.join("worktrees");
    watch_directory(&worktrees, RecursiveMode::NonRecursive, watches);
    if let Ok(entries) = std::fs::read_dir(&worktrees) {
        for entry in entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let path = entry.path();
            directories.insert(path.clone());
            for name in ["gitdir", "commondir", "locked"] {
                record(path.join(name), &mut snapshot.worktrees);
            }
            // Direct deletion of a checkout does not touch Git metadata.
            if let Ok(gitdir) = std::fs::read_to_string(path.join("gitdir")) {
                let gitdir = PathBuf::from(gitdir.trim_end_matches(['\r', '\n']));
                snapshot
                    .worktrees
                    .insert(gitdir.clone(), u64::from(gitdir.exists()));
                if let Some(root) = gitdir.parent() {
                    snapshot.roots.insert(root.to_owned());
                }
            }
        }
    }
    for directory in directories {
        watch_directory(&directory, RecursiveMode::NonRecursive, watches);
        if let Some(parent) = directory.parent() {
            watch_directory(parent, RecursiveMode::NonRecursive, watches);
        }
        for name in [
            "HEAD",
            "config",
            "config.worktree",
            "packed-refs",
            "shallow",
            "commondir",
        ] {
            record(directory.join(name), &mut snapshot.history);
        }
        // Symbolic HEAD stays unchanged on ordinary commits, but changes on
        // checkout. That distinction keeps commits from reloading the sidebar.
        let head = directory.join("HEAD");
        snapshot
            .worktrees
            .insert(head.clone(), snapshot.history[&head]);
        for name in ["refs", "reftable"] {
            let path = directory.join(name);
            watch_directory(&path, RecursiveMode::Recursive, watches);
            record_tree(&path, &mut snapshot.history);
        }
    }
    for root in &snapshot.roots {
        snapshot
            .worktrees
            .insert(root.clone(), u64::from(root.is_dir()));
    }
    snapshot
}

pub(super) fn collect_all(
    repositories: &BTreeMap<PathBuf, Repository>,
) -> (BTreeMap<PathBuf, Metadata>, Watches) {
    let mut watches = Watches::new();
    let snapshots = repositories
        .iter()
        .map(|(root, repository)| (root.clone(), collect(repository, &mut watches)))
        .collect();
    (snapshots, watches)
}
