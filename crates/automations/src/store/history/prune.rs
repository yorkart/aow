use std::{
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
};

use anyhow::Result;

use super::super::{FileLock, Store, try_exclusive, valid_component};
use super::order::compare_ids;

impl Store {
    /// Removes old completed journals while retaining the newest `retain` runs
    /// for every task. A journal whose writer still holds its advisory lock is
    /// left in place, even when it is older than the retention boundary.
    pub fn prune_run_history(&self, retain: usize) -> Result<usize> {
        self.prune_run_history_if(retain, |_, _| true)
    }

    /// Let consumers retain journals they have not processed yet. Active
    /// journals are still protected by their writer lock independently.
    pub fn prune_run_history_if(
        &self,
        retain: usize,
        can_remove: impl Fn(&str, &str) -> bool,
    ) -> Result<usize> {
        let runs_root = self.root.join("runs");
        let mut removed = 0;
        for entry in fs::read_dir(runs_root)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let task_id = entry.file_name().to_string_lossy().into_owned();
            if valid_component(&task_id).is_err() {
                continue;
            }
            removed += self.prune_task_run_history(&task_id, retain, &can_remove)?;
        }
        Ok(removed)
    }

    fn prune_task_run_history(
        &self,
        task_id: &str,
        retain: usize,
        can_remove: &impl Fn(&str, &str) -> bool,
    ) -> Result<usize> {
        let directory = self.root.join("runs").join(task_id);
        let mut run_ids = Vec::new();
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let Some(run_id) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if valid_component(run_id).is_ok() {
                run_ids.push(run_id.to_owned());
            }
        }
        run_ids.sort_unstable_by(|a, b| compare_ids(b, a));

        let mut removed = 0;
        for run_id in run_ids.into_iter().skip(retain) {
            if !can_remove(task_id, &run_id) {
                continue;
            }
            let Some(file) = self.lock_inactive_run(task_id, &run_id)? else {
                continue;
            };
            if self.remove_locked_run(task_id, &run_id, file)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn lock_inactive_run(&self, task_id: &str, run_id: &str) -> Result<Option<FileLock>> {
        let path = self.run_events_path(task_id, run_id)?;
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !file.metadata()?.file_type().is_file() || !try_exclusive(&file)? {
            return Ok(None);
        }
        Ok(Some(FileLock {
            file,
            owns_lock: true,
        }))
    }

    fn remove_locked_run(&self, task_id: &str, run_id: &str, _lock: FileLock) -> Result<bool> {
        let directory = self.run_path(task_id, run_id)?;
        // A process killed before RunWriter::drop leaves this hard link behind.
        // Once the journal lock is available, it is no longer an active run and
        // both names must be removed to release the inode.
        let active_path = directory.parent().unwrap().join(".active").join(run_id);
        match fs::remove_file(active_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match fs::remove_dir_all(directory) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}
