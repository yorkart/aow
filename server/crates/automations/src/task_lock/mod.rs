//! Server scheduling slots retained while a remote execution is unfinished.

#[cfg(test)]
mod tests;

use crate::{
    Store,
    store::{private_dir, try_exclusive, valid_component},
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
};

struct TaskLock {
    file: File,
}

/// A scheduler slot remains reserved until its node execution is reconciled.
pub(crate) struct ConcurrencySlot {
    lock: TaskLock,
}

#[derive(Deserialize)]
struct Owner {
    run_id: String,
}

impl Store {
    pub(crate) fn concurrency_slot(
        &self,
        task_id: &str,
        max_concurrent_runs: u8,
    ) -> Result<Option<ConcurrencySlot>> {
        valid_component(task_id)?;
        ensure!(
            (1..=10).contains(&max_concurrent_runs),
            aow_i18n::text("automations.maximum_concurrency_must_be_between_1_and_10")
        );
        for slot in 1..=max_concurrent_runs {
            if let Some(lock) = self.try_task_lock(task_id, &format!("slot-{slot}.lock"))? {
                return Ok(Some(ConcurrencySlot { lock }));
            }
        }
        Ok(None)
    }

    /// Check the caller lock and durable remote execution reference.
    pub(crate) fn has_occupied_concurrency_slot(&self, task_id: &str) -> Result<bool> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id);
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("slot-") && name.ends_with(".lock"))
            {
                continue;
            }
            let file = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            ensure!(
                file.metadata()?.is_file(),
                aow_i18n::text("common.lockNotFile")
            );
            let mut contents = String::new();
            (&file).take(64 * 1024 + 1).read_to_string(&mut contents)?;
            ensure!(
                contents.len() <= 64 * 1024,
                aow_i18n::text("common.lockTooLarge")
            );
            let lines = contents
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'));
            let Some(first) = lines.into_iter().next() else {
                continue;
            };
            // Diagnostic metadata can be empty, partial, or stale during a handoff.
            let Ok(owner) = serde_json::from_str::<Owner>(first) else {
                continue;
            };
            if valid_component(&owner.run_id).is_err() {
                continue;
            }
            match self.read_run(task_id, &owner.run_id) {
                Ok(Some(run)) if run.finished_at.is_some() => continue,
                Ok(_) => {}
                Err(error) if crate::store::is_not_found(&error) => {}
                Err(error) => return Err(error),
            }
            if !try_exclusive(&file)?
                || self
                    .run_path(task_id, &owner.run_id)?
                    .join("node-execution.json")
                    .is_file()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn try_task_lock(&self, task_id: &str, name: &str) -> Result<Option<TaskLock>> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id);
        private_dir(&directory)?;
        self.try_task_lock_path(task_id, &directory.join(name))
    }

    fn try_task_lock_path(
        &self,
        task_id: &str,
        path: &std::path::Path,
    ) -> Result<Option<TaskLock>> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        if !try_exclusive(&file)? {
            return Ok(None);
        }
        ensure!(
            file.metadata()?.len() <= 64 * 1024,
            aow_i18n::text("common.lockTooLarge")
        );
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let mut lines = contents.lines();
        if let Some(first) = lines.next()
            && let Ok(owner) = serde_json::from_str::<Owner>(first)
            && self
                .run_path(task_id, &owner.run_id)?
                .join("node-execution.json")
                .is_file()
            && self
                .read_run(task_id, &owner.run_id)?
                .is_some_and(|run| !run.status.terminal())
        {
            return Ok(None);
        }
        Ok(Some(TaskLock { file }))
    }
}

impl TaskLock {
    fn begin(&mut self, run_id: &str) -> Result<()> {
        self.file.set_len(0)?;
        serde_json::to_writer(
            &mut self.file,
            &serde_json::json!({
                "runner_pid": std::process::id(), "run_id": run_id
            }),
        )?;
        self.file.write_all(b"\n")?;
        self.file.sync_data()?;
        Ok(())
    }
}

impl ConcurrencySlot {
    pub fn begin(&mut self, run_id: &str) -> Result<()> {
        self.lock.begin(run_id)
    }
}
