//! Server-owned observation of durable run results; the runner never delivers IM.
use std::{
    fs::{self, File, OpenOptions},
    future::Future,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use aow_automations::{
    FailureNotification, Run, RunStatus, Store,
    store::{atomic_write, valid_component},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const STATE_FILE: &str = "failure-notification.json";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeliveryStatus {
    NotRequested,
    Sending,
    Sent,
    SkippedMissingBot,
    Failed,
    Unconfirmed,
}

#[derive(Serialize, Deserialize)]
struct DeliveryState {
    status: DeliveryStatus,
    delivery_id: Option<String>,
    updated_at: DateTime<Utc>,
    message: Option<String>,
}

fn save(
    path: &Path,
    status: DeliveryStatus,
    delivery_id: Option<String>,
    message: Option<String>,
) -> Result<()> {
    atomic_write(
        path,
        &serde_json::to_vec(&DeliveryState {
            status,
            delivery_id,
            updated_at: Utc::now(),
            message,
        })?,
    )
}

fn directories(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if valid_component(&id).is_ok() && entry.file_type()?.is_dir() {
            result.push((id, entry.path()));
        }
    }
    result.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

fn candidate(store: &Store, task_id: &str, run_id: &str, directory: &Path) -> Result<Option<Run>> {
    let path = directory.join(STATE_FILE);
    match fs::read(&path) {
        Ok(bytes) => {
            let state: DeliveryState = serde_json::from_slice(&bytes)?;
            if state.status == DeliveryStatus::Sending {
                // The previous server may have sent successfully and stopped
                // before recording the response. Never replay an ambiguous send.
                save(
                    &path,
                    DeliveryStatus::Unconfirmed,
                    state.delivery_id,
                    Some(
                        "发送过程中 Server 退出，无法确认发送结果；为避免重复提醒，不自动重发"
                            .into(),
                    ),
                )?;
            }
            return Ok(None);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let Some(detail) = store.read_run_detail(task_id, run_id)? else {
        return Ok(None);
    };
    if !detail.run.status.terminal() {
        return Ok(None);
    }
    if detail.run.status == RunStatus::Failed
        && detail.configuration.failure_notification == Some(FailureNotification::Feishu)
    {
        return Ok(Some(detail.run));
    }
    save(&path, DeliveryStatus::NotRequested, None, None)?;
    Ok(None)
}

struct ScanLock(File);

impl Drop for ScanLock {
    fn drop(&mut self) {
        // A concurrent fork can inherit this file description until exec.
        // Closing our copy alone would keep the next observer locked out.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

struct Scan {
    // A single observer owns scanning, delivery and pruning, even if two server
    // processes temporarily overlap. This file is never unlinked or renamed.
    _lock: ScanLock,
    pending: Vec<(PathBuf, Run)>,
}

fn scan(store: &Store) -> Result<Option<Scan>> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(store.root.join("failure-notifications.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        return Err(error.into());
    }
    let lock = ScanLock(lock);
    let mut pending = Vec::new();
    // Walk local run journals, including runs whose task was deleted or whose
    // config repository changed. Each journal contains its own config snapshot.
    for (task_id, directory) in directories(&store.root.join("runs"))? {
        let runs = match directories(&directory) {
            Ok(runs) => runs,
            Err(error) => {
                tracing::warn!(%task_id, %error, "cannot scan automation runs for failure notifications");
                continue;
            }
        };
        for (run_id, directory) in runs {
            match candidate(store, &task_id, &run_id, &directory) {
                Ok(Some(run)) => pending.push((directory.join(STATE_FILE), run)),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%task_id, %run_id, %error, "cannot inspect automation failure notification")
                }
            }
        }
    }
    Ok(Some(Scan {
        _lock: lock,
        pending,
    }))
}

pub(super) async fn poll<F, Fut>(store: Arc<Store>, send: F, prune: bool) -> Result<()>
where
    F: Fn(Run, String) -> Fut,
    Fut: Future<Output = Result<bool>>,
{
    let scan_store = store.clone();
    let Some(scan) = tokio::task::spawn_blocking(move || scan(&scan_store)).await?? else {
        return Ok(());
    };
    for (path, run) in scan.pending {
        let delivery_id = Uuid::new_v4().to_string();
        let claim_path = path.clone();
        let claim_id = delivery_id.clone();
        if let Err(error) = tokio::task::spawn_blocking(move || {
            save(&claim_path, DeliveryStatus::Sending, Some(claim_id), None)
        })
        .await?
        {
            tracing::warn!(task_id = %run.task_id, run_id = %run.id, %error, "cannot persist automation notification attempt");
            continue;
        }
        let (status, message) = match send(run.clone(), delivery_id.clone()).await {
            Ok(true) => (DeliveryStatus::Sent, None),
            Ok(false) => (
                DeliveryStatus::SkippedMissingBot,
                Some("当前环境未配置飞书 Bot，跳过本次提醒".into()),
            ),
            Err(error) => (DeliveryStatus::Failed, Some(format!("{error:#}"))),
        };
        tracing::info!(task_id = %run.task_id, run_id = %run.id, ?status, ?message, "automation failure notification processed");
        if let Err(error) =
            tokio::task::spawn_blocking(move || save(&path, status, Some(delivery_id), message))
                .await?
        {
            tracing::warn!(task_id = %run.task_id, run_id = %run.id, %error, "cannot persist automation notification result");
        }
    }
    // Offline failures must be observed before the normal retention policy can
    // remove old journals. Delivery state is removed along with its run directory.
    if prune {
        tokio::task::spawn_blocking(move || {
            store.prune_run_history_if(super::RUN_HISTORY_RETENTION, |task_id, run_id| {
                // A run may finish after scanning, while another notification
                // is being sent. Keep it until a later poll has processed it.
                let path = store
                    .root
                    .join("runs")
                    .join(task_id)
                    .join(run_id)
                    .join(STATE_FILE);
                fs::read(path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<DeliveryState>(&bytes).ok())
                    .is_some_and(|state| state.status != DeliveryStatus::Sending)
            })
        })
        .await??;
    }
    drop(scan._lock);
    Ok(())
}

#[cfg(test)]
mod tests;
