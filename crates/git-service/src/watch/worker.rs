use notify::{RecommendedWatcher, Watcher};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

use super::{
    GitChanges,
    metadata::{Metadata, Watches, collect_all},
    repository::Repository,
};

const SETTLE: Duration = Duration::from_millis(250);
const RECONCILE: Duration = Duration::from_secs(30);

pub(super) async fn run(
    mut roots: watch::Receiver<BTreeSet<PathBuf>>,
    changes: watch::Sender<GitChanges>,
) {
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
