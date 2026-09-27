//! Git metadata watching, independent of project registration and HTTP.
//! Events invalidate worktree/history snapshots. A metadata-only sweep also
//! recovers missed native events without periodically running `git log`.

use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, watch};

use super::{GitError, SMALL_OUTPUT_LIMIT, absolute, run_git};

const SETTLE: Duration = Duration::from_millis(250);
const RECONCILE: Duration = Duration::from_secs(30);

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
        let task = tokio::spawn(run(receiver, sender));
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

async fn metadata_output(root: &Path, args: &[&str]) -> Option<String> {
    // Reuse the service's timeout, output limits and GIT_OPTIONAL_LOCKS policy.
    let output = run_git(root, args, SMALL_OUTPUT_LIMIT).await.ok()?;
    if output.truncated {
        return None;
    }
    String::from_utf8(output.bytes).ok()
}

#[derive(Clone)]
struct Repository {
    root: PathBuf,
    git: PathBuf,
    common: PathBuf,
    main: PathBuf,
}

impl Repository {
    async fn resolve(root: &Path) -> Option<Self> {
        let text = metadata_output(root, &["rev-parse", "--git-dir", "--git-common-dir"]).await?;
        let mut lines = text.lines();
        let resolve = |line: &str| std::fs::canonicalize(root.join(line)).ok();
        let git = resolve(lines.next()?)?;
        let common = resolve(lines.next()?)?;
        // The first entry is the main checkout, also when the supplied root
        // is a linked worktree or uses --separate-git-dir.
        let listing = metadata_output(root, &["worktree", "list", "--porcelain", "-z"]).await?;
        let main = PathBuf::from(listing.split('\0').next()?.strip_prefix("worktree ")?);
        Some(Self {
            root: root.to_owned(),
            git,
            common,
            main,
        })
    }

    fn affected_by(&self, path: &Path) -> bool {
        [&self.git, &self.common].iter().any(|directory| {
            let Ok(relative) = path.strip_prefix(directory) else {
                return false;
            };
            let parts: Vec<_> = relative.iter().filter_map(|part| part.to_str()).collect();
            match parts.as_slice() {
                [] => true,
                [
                    "HEAD" | "config" | "config.worktree" | "packed-refs" | "shallow" | "commondir",
                ] => true,
                ["refs" | "reftable", ..] => !path.to_string_lossy().ends_with(".lock"),
                ["worktrees"] | ["worktrees", _] => true,
                [
                    "worktrees",
                    _,
                    "HEAD" | "gitdir" | "commondir" | "locked" | "config.worktree",
                ] => true,
                ["worktrees", _, "refs" | "reftable", ..] => {
                    !path.to_string_lossy().ends_with(".lock")
                }
                _ => false,
            }
        })
    }
}

type Fingerprint = BTreeMap<PathBuf, u64>;
type Watches = BTreeMap<PathBuf, RecursiveMode>;

#[derive(Default, PartialEq, Eq, Debug)]
struct Metadata {
    history: Fingerprint,
    worktrees: Fingerprint,
    roots: BTreeSet<PathBuf>,
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

fn collect(repository: &Repository, watches: &mut Watches) -> Metadata {
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

fn collect_all(
    repositories: &BTreeMap<PathBuf, Repository>,
) -> (BTreeMap<PathBuf, Metadata>, Watches) {
    let mut watches = Watches::new();
    let snapshots = repositories
        .iter()
        .map(|(root, repository)| (root.clone(), collect(repository, &mut watches)))
        .collect();
    (snapshots, watches)
}

async fn run(mut roots: watch::Receiver<BTreeSet<PathBuf>>, changes: watch::Sender<GitChanges>) {
    // Bounded native event queue; the metadata sweep also covers overflow.
    let (events, mut notifications) = mpsc::channel(64);
    let failed = Arc::new(AtomicBool::new(false));
    let create_watcher = || {
        let events = events.clone();
        let failed = failed.clone();
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.as_ref().is_ok_and(|event| matches!(event.kind, notify::EventKind::Access(_))) { return; }
            match event {
                Ok(event) => { let _ = events.try_send(event); }
                Err(error) => {
                    failed.store(true, Ordering::Relaxed);
                    tracing::warn!(%error, "Git metadata watcher failed; reconciliation remains active");
                    let _ = events.try_send(notify::Event::new(notify::EventKind::Any));
                }
            }
        }).map_err(|error| tracing::warn!(%error, "Git metadata watcher unavailable; using reconciliation")).ok()
    };
    let mut watcher: Option<RecommendedWatcher> = create_watcher();
    let mut repositories = BTreeMap::<PathBuf, Repository>::new();
    let mut snapshots = BTreeMap::<PathBuf, Metadata>::new();
    let mut watching = Watches::new();
    let mut interval = tokio::time::interval(RECONCILE);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            changed = roots.changed() => { if changed.is_err() { break; } },
            Some(event) = notifications.recv() => {
                let mut batch = vec![event];
                // Fixed batch window also bounds latency during a continuous rebase.
                tokio::time::sleep(SETTLE).await;
                while let Ok(event) = notifications.try_recv() { batch.push(event); }
                // Reattach only after watcher errors or watched directory
                // replacement, rather than restarting healthy watches on a timer.
                if batch.iter().any(|event| event.need_rescan() ||
                    (matches!(event.kind, notify::EventKind::Remove(_) | notify::EventKind::Modify(notify::event::ModifyKind::Name(_)))
                        && event.paths.iter().any(|path| watching.contains_key(path)))) {
                    failed.store(true, Ordering::Relaxed);
                }
                if !failed.load(Ordering::Relaxed) && !batch.iter().any(|event|
                    event.paths.is_empty() || event.paths.iter().any(|path|
                        repositories.values().any(|repository| repository.affected_by(path)))) {
                    continue;
                }
            },
            _ = interval.tick() => {},
        }
        if failed.swap(false, Ordering::Relaxed) || watcher.is_none() {
            drop(watcher.take());
            watching.clear();
            watcher = create_watcher();
        }
        let roots = roots.borrow_and_update().clone();
        repositories.retain(|root, _| roots.contains(root));
        for root in roots {
            if !repositories.contains_key(&root)
                && let Some(repository) = Repository::resolve(&root).await
            {
                repositories.insert(root, repository);
            }
        }
        let mut next = BTreeMap::new();
        let mut planned = None;
        // A new worktree can appear while we are attaching watches. Publish
        // its snapshot only after attaching its newly discovered metadata
        // directories and rereading them; otherwise its next move is missed.
        for attempt in 0..3 {
            let scan = repositories.clone();
            let Ok((current, desired)) =
                tokio::task::spawn_blocking(move || collect_all(&scan)).await
            else {
                return;
            };
            next = current;
            if desired == watching || planned.as_ref() == Some(&desired) || watcher.is_none() {
                break;
            }
            planned = Some(desired.clone());
            if let Some(watcher) = &mut watcher {
                // FSEvents restarts its stream on path changes; batch them to
                // minimize the gap, then read metadata again below.
                let mut paths = watcher.paths_mut();
                watching.retain(|path, mode| {
                    if desired.get(path) == Some(mode) {
                        return true;
                    }
                    let _ = paths.remove(path);
                    false
                });
                for (path, mode) in desired {
                    if !watching.contains_key(&path) {
                        match paths.add(&path, mode) {
                            Ok(()) => {
                                watching.insert(path, mode);
                            }
                            Err(error) => {
                                tracing::debug!(%error, path = %path.display(), "Will retry Git metadata watch")
                            }
                        }
                    }
                }
                if let Err(error) = paths.commit() {
                    failed.store(true, Ordering::Relaxed);
                    tracing::warn!(%error, "Failed to update Git metadata watches");
                }
            }
            if attempt == 2 {
                let _ = events.try_send(notify::Event::new(notify::EventKind::Any));
            }
        }
        let changed: Vec<_> = next
            .iter()
            .filter(|(root, value)| snapshots.get(*root) != Some(*value))
            .flat_map(|(_, value)| value.roots.iter().cloned())
            .collect();
        let removed = snapshots.keys().any(|root| !next.contains_key(root));
        let worktrees_changed = removed
            || next.iter().any(|(root, value)| {
                snapshots.get(root).is_none_or(|previous| {
                    previous.worktrees != value.worktrees || previous.roots != value.roots
                })
            });
        if !changed.is_empty() || removed {
            let roots: BTreeSet<_> = next.values().flat_map(|value| value.roots.iter()).collect();
            changes.send_modify(|snapshot| {
                snapshot.revision += 1;
                if worktrees_changed {
                    snapshot.worktrees = snapshot.revision;
                }
                snapshot.repositories.retain(|root, _| roots.contains(root));
                for root in changed {
                    snapshot.repositories.insert(root, snapshot.revision);
                }
            });
        }
        snapshots = next;
    }
}

#[cfg(test)]
mod tests;
