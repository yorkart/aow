//! Bridges registered project roots and Git changes to workspace event streams.

use std::{
    collections::BTreeMap,
    convert::Infallible,
    path::Path,
    sync::{Arc, Mutex},
};

use aow_git_service::{GitChanges, GitWatcher};
use axum::{
    Router,
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
};
use serde::Serialize;
use tokio::sync::{Notify, watch};

use crate::{AppState, aow::AowManager};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Snapshot {
    boot_id: String,
    revision: u64,
    projects: u64,
    terminals: u64,
    repositories: BTreeMap<String, u64>,
}

struct Inner {
    changes: watch::Sender<Snapshot>,
    reconcile: Arc<Notify>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().unwrap().take() {
            task.abort();
        }
    }
}

#[derive(Clone)]
pub(crate) struct WorkspaceEvents(Arc<Inner>);

impl WorkspaceEvents {
    pub(crate) fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.0.changes.subscribe()
    }

    pub(crate) fn new() -> Self {
        Self(Arc::new(Inner {
            changes: watch::channel(Snapshot {
                boot_id: uuid::Uuid::new_v4().to_string(),
                revision: 0,
                projects: 0,
                terminals: 0,
                repositories: BTreeMap::new(),
            })
            .0,
            reconcile: Arc::new(Notify::new()),
            task: Mutex::new(None),
        }))
    }

    pub(crate) fn projects_changed(&self) {
        self.0.changes.send_modify(|snapshot| {
            snapshot.revision += 1;
            snapshot.projects = snapshot.revision;
        });
        self.reconcile();
    }

    pub(crate) fn terminals_changed(&self) {
        self.0.changes.send_modify(|snapshot| {
            snapshot.revision += 1;
            snapshot.terminals = snapshot.revision;
        });
        // A CLI may have prepared a worktree just before creating the pane.
        self.reconcile();
    }

    fn reconcile(&self) {
        self.0.reconcile.notify_one();
    }

    pub(crate) fn start(&self, aow: AowManager) {
        let mut task = self.0.task.lock().unwrap();
        if task.is_none() {
            *task = Some(tokio::spawn(run(
                aow,
                self.0.changes.clone(),
                self.0.reconcile.clone(),
            )));
        }
    }
}

async fn run(aow: AowManager, changes: watch::Sender<Snapshot>, reconcile: Arc<Notify>) {
    let roots = match aow.repository_roots() {
        Ok(roots) => roots,
        Err(error) => {
            tracing::warn!(%error, "Could not load repositories for Git watching");
            return;
        }
    };
    let watcher = match GitWatcher::new(roots) {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(%error, "Could not start Git watching");
            return;
        }
    };
    let mut receiver = watcher.subscribe();
    let mut previous = GitChanges::default();
    loop {
        tokio::select! {
            _ = reconcile.notified() => {
                if let Ok(roots) = aow.repository_roots()
                    && let Err(error) = watcher.set_repositories(roots) {
                    tracing::warn!(%error, "Could not update Git watch roots");
                }
            },
            changed = receiver.changed() => {
                if changed.is_err() { return; }
                let next = receiver.borrow_and_update().clone();
                changes.send_modify(|snapshot| {
                    snapshot.revision += 1;
                    if previous.worktrees != next.worktrees {
                        snapshot.projects = snapshot.revision;
                    }
                    snapshot.repositories.retain(|root, _| next.repositories.contains_key(Path::new(root)));
                    for (root, revision) in &next.repositories {
                        if previous.repositories.get(root) != Some(revision) {
                            snapshot.repositories.insert(root.to_string_lossy().into_owned(), snapshot.revision);
                        }
                    }
                });
                previous = next;
            },
        }
    }
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route("/api/workspace/events", get(stream))
}

async fn stream(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let stream = futures_util::stream::unfold(
        (state.workspace_events.0.changes.subscribe(), true),
        |(mut receiver, initial)| async move {
            if !initial && receiver.changed().await.is_err() {
                return None;
            }
            let snapshot = receiver.borrow_and_update().clone();
            let event = Event::default()
                .event("workspace")
                .json_data(snapshot)
                .expect("serializable snapshot");
            Some((Ok(event), (receiver, false)))
        },
    );
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests;
