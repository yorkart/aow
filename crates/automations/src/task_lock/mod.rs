//! Stable per-task concurrency slots with process-group ownership tracking.

mod process;

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

use process::{boot_identity, group_alive};

struct TaskLock {
    file: File,
}

/// A stable task-local concurrency slot. The slot stays reserved by every
/// process group the runner starts, so a crashed runner cannot immediately
/// allow an orphaned Agent to be exceeded.
pub(crate) struct ConcurrencySlot {
    lock: TaskLock,
}

#[derive(Deserialize)]
struct Owner {
    boot: String,
    run_id: String,
}

#[derive(Deserialize)]
struct Process {
    process_group: i32,
    started_before: u64,
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
            "最大同时执行数必须为 1–10"
        );
        for slot in 1..=max_concurrent_runs {
            if let Some(lock) = self.try_task_lock(task_id, &format!("slot-{slot}.lock"))? {
                return Ok(Some(ConcurrencySlot { lock }));
            }
        }
        Ok(None)
    }

    /// Observe registered process groups without ever acquiring a runner slot.
    /// The active journal covers the runner; these records cover orphaned children.
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
            ensure!(file.metadata()?.is_file(), "任务锁不是普通文件");
            let mut contents = String::new();
            file.take(64 * 1024 + 1).read_to_string(&mut contents)?;
            ensure!(contents.len() <= 64 * 1024, "任务锁信息过大");
            let mut lines = contents
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'));
            let Some(first) = lines.next() else {
                continue;
            };
            // Diagnostic metadata can be empty, partial, or stale during a handoff.
            let Ok(owner) = serde_json::from_str::<Owner>(first) else {
                continue;
            };
            if valid_component(&owner.run_id).is_err() || owner.boot != boot_identity()? {
                continue;
            }
            match self.read_run(task_id, &owner.run_id) {
                Ok(Some(run)) if run.finished_at.is_some() => continue,
                Ok(_) => {}
                Err(error) if crate::store::is_not_found(&error) => {}
                Err(error) => return Err(error),
            }
            for line in lines {
                if let Ok(process) = serde_json::from_str::<Process>(line)
                    && group_alive(&process)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn try_task_lock(&self, task_id: &str, name: &str) -> Result<Option<TaskLock>> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id);
        private_dir(&directory)?;
        self.try_task_lock_path(&directory.join(name))
    }

    fn try_task_lock_path(&self, path: &std::path::Path) -> Result<Option<TaskLock>> {
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
        ensure!(file.metadata()?.len() <= 64 * 1024, "任务锁信息过大");
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let mut lines = contents.lines();
        if let Some(first) = lines.next()
            && let Ok(owner) = serde_json::from_str::<Owner>(first)
            && owner.boot == boot_identity()?
        {
            for line in lines {
                // A partial final registration cannot have reached exec.
                if let Ok(process) = serde_json::from_str::<Process>(line)
                    && group_alive(&process)?
                {
                    return Ok(None);
                }
            }
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
                "runner_pid": std::process::id(), "run_id": run_id, "boot": boot_identity()?
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
