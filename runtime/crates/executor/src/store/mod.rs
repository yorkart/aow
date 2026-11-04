mod session_roots;
use crate::{RunOutput, Task};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
pub const MAX_RUN_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct Store {
    pub(crate) root: PathBuf,
    pub(crate) config_dir: PathBuf,
}
pub(crate) struct RunWriter {
    file: File,
    directory: PathBuf,
    failed: bool,
}
pub(crate) fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "Invalid execution ID"
    );
    Ok(())
}
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
impl Store {
    pub(crate) fn new(root: &Path) -> Result<Self> {
        let directory = root.join("data/executions");
        std::fs::create_dir_all(&directory)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        Ok(Self {
            root: directory,
            config_dir: root.to_path_buf(),
        })
    }
    pub(crate) fn run_path(&self, _: &str, id: &str) -> Result<PathBuf> {
        valid_id(id)?;
        Ok(self.root.join(id))
    }
    pub(crate) fn create(&self, task: &Task) -> Result<RunWriter> {
        let directory = self.run_path("", &task.id)?;
        std::fs::create_dir(&directory)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        atomic_write(&directory.join("spec.json"), &serde_json::to_vec(task)?)?;
        for name in ["stdio", "stderr"] {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(directory.join(name))?;
        }
        let file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join("events.jsonl"))?;
        Ok(RunWriter {
            file,
            directory,
            failed: false,
        })
    }
    pub(crate) fn write_run_result(
        &self,
        task: &str,
        id: &str,
        result: Result<String>,
    ) -> Result<()> {
        let value = match result {
            Ok(conclusion) => serde_json::json!({"status":"completed","conclusion":conclusion}),
            Err(error) => serde_json::json!({"status":"unavailable","error":format!("{error:#}")}),
        };
        atomic_write(
            &self.run_path(task, id)?.join("result.json"),
            &serde_json::to_vec(&value)?,
        )
    }
}
impl RunWriter {
    pub(crate) fn append(&mut self, event: &impl Serialize) -> Result<()> {
        ensure!(!self.failed, "Execution journal previously failed");
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        let result = (|| {
            self.file.write_all(&bytes)?;
            self.file.sync_data()
        })();
        if result.is_err() {
            self.failed = true;
        }
        result.context("Cannot append execution event")
    }
    pub(crate) fn output_writer(&self, output: RunOutput) -> Result<File> {
        Ok(OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.directory.join(output.filename()))?)
    }
}
