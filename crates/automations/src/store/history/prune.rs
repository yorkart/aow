use std::{
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
};

use anyhow::Result;

use super::super::{FileLock, Store, try_exclusive, valid_component};

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
        self.prune_history(retain, can_remove, |_, _, error| Err(error))
    }

    /// Continue cleaning healthy history when individual journals or tasks fail.
    /// Unreadable headers are retained for diagnosis, never guessed to be old.
    /// The callback receives an empty run ID for task-level failures. Errors
    /// reading the history root still propagate to the caller.
    pub fn prune_run_history_with_errors(
        &self,
        retain: usize,
        can_remove: impl Fn(&str, &str) -> bool,
        mut on_error: impl FnMut(&str, &str, anyhow::Error),
    ) -> Result<usize> {
        self.prune_history(retain, can_remove, |task_id, run_id, error| {
            on_error(task_id, run_id, error);
            Ok(())
        })
    }

    fn prune_history(
        &self,
        retain: usize,
        can_remove: impl Fn(&str, &str) -> bool,
        mut on_error: impl FnMut(&str, &str, anyhow::Error) -> Result<()>,
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
            match self.prune_task_run_history(&task_id, retain, &can_remove, &mut on_error) {
                Ok(count) => removed += count,
                Err(error) => on_error(&task_id, "", error)?,
            }
        }
        Ok(removed)
    }

    fn prune_task_run_history(
        &self,
        task_id: &str,
        retain: usize,
        can_remove: &impl Fn(&str, &str) -> bool,
        on_error: &mut impl FnMut(&str, &str, anyhow::Error) -> Result<()>,
    ) -> Result<usize> {
        let run_ids = self.run_ids_with_errors(task_id, None, |run_id, error| {
            on_error(task_id, run_id, error)
        })?;

        let mut removed = 0;
        for run_id in run_ids.into_iter().skip(retain) {
            if !can_remove(task_id, &run_id) {
                continue;
            }
            let result = (|| {
                let Some(file) = self.lock_inactive_run(task_id, &run_id)? else {
                    return Ok(false);
                };
                self.remove_locked_run(task_id, &run_id, file)
            })();
            match result {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(error) => on_error(task_id, &run_id, error)?,
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
