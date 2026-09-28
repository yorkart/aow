use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
};

use anyhow::Result;
use serde::Serialize;

use crate::RunOutput;

pub struct RunWriter {
    pub(super) file: File,
    pub(super) directory: PathBuf,
    pub(super) active_path: Option<PathBuf>,
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
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn output_writer(&self, output: RunOutput) -> Result<File> {
        Ok(OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.directory.join(output.filename()))?)
    }
}
