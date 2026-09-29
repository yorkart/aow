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
        if let Some(path) = &self.active_path {
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
            bail!("执行记录此前写入失败，已停止追加: {error}");
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
        result.context("写入执行记录失败，已停止追加")
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
