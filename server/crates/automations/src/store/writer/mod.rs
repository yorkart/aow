use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::RunOutput;

pub struct RunWriter {
    pub(super) file: File,
    pub(super) directory: PathBuf,
    pub(super) active_path: Option<PathBuf>,
    pub(super) failure: Option<String>,
}
impl Drop for RunWriter {
    fn drop(&mut self) {
        let remote_pending = self.directory.join("node-execution.json").is_file()
            && fs::read(self.directory.join("events.jsonl")).is_ok_and(|bytes| {
                !bytes
                    .split_inclusive(|byte| *byte == b'\n')
                    .filter(|line| line.ends_with(b"\n"))
                    .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
                    .any(|event| event["type"] == "finished")
            });
        if !remote_pending && let Some(path) = &self.active_path {
            let _ = fs::remove_file(path);
        }
        // Concurrent fork/exec can briefly inherit this file description despite
        // CLOEXEC. Unlock explicitly so a finished owner is immediately observable.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
impl RunWriter {
    pub fn append(&mut self, event: &impl Serialize) -> Result<()> {
        self.append_with(event, |file, line| {
            file.write_all(line)?;
            file.sync_data()
        })
    }

    fn append_with(
        &mut self,
        event: &impl Serialize,
        persist: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<()> {
        if let Some(error) = &self.failure {
            bail!(aow_i18n::message(
                "automations.run_record_previously_failed_to_write_appending_stopped",
                &[("p0", error.to_string())]
            ));
        }
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        let result = persist(&mut self.file, &line);
        // write_all may have persisted only a prefix. Even if space becomes
        // available, appending Finished would join it to the torn JSON record.
        // A sync failure is also ambiguous, so never reuse this writer after I/O errors.
        if let Err(error) = &result {
            // The runner may report only the error from its final append.
            self.failure = Some(error.to_string());
        }
        result.context(aow_i18n::text(
            "automations.failed_to_write_run_record_appending_stopped",
        ))
    }

    pub fn output_writer(&self, output: RunOutput) -> Result<File> {
        Ok(OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.directory.join(output.filename()))?)
    }
}

#[cfg(test)]
mod tests;

impl super::Store {
    pub(crate) fn resume_run_writer(&self, task_id: &str, id: &str) -> Result<Option<RunWriter>> {
        let directory = self.run_path(task_id, id)?;
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(directory.join("events.jsonl"))?;
        if !super::try_exclusive(&file)? {
            return Ok(None);
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        (&mut file).take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 1024 * 1024, "Run journal exceeds limit");
        let end = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        file.set_len(end as u64)?;
        let active = directory.parent().unwrap().join(".active").join(id);
        Ok(Some(RunWriter {
            file,
            directory,
            active_path: Some(active),
            failure: None,
        }))
    }
}
