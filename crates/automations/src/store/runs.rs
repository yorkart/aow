use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};

use anyhow::{Result, ensure};

use crate::{Run, RunEvent, RunOutput, Task};

use super::{RunWriter, Store, private_dir, try_exclusive, valid_component};

pub const MAX_RUN_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;

impl Store {
    pub fn run_path(&self, task_id: &str, run_id: &str) -> Result<PathBuf> {
        valid_component(task_id)?;
        valid_component(run_id)?;
        Ok(self.root.join("runs").join(task_id).join(run_id))
    }

    pub(super) fn run_events_path(&self, task_id: &str, run_id: &str) -> Result<PathBuf> {
        Ok(self.run_path(task_id, run_id)?.join("events.jsonl"))
    }

    pub fn run_output_path(
        &self,
        task_id: &str,
        run_id: &str,
        output: RunOutput,
    ) -> Result<PathBuf> {
        Ok(self.run_path(task_id, run_id)?.join(output.filename()))
    }

    pub fn read_run_output(
        &self,
        task_id: &str,
        run_id: &str,
        output: RunOutput,
    ) -> Result<Vec<u8>> {
        let path = self.run_output_path(task_id, run_id, output)?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let metadata = file.metadata()?;
        ensure!(metadata.file_type().is_file(), "执行输出文件无效");
        ensure!(
            metadata.len() <= MAX_RUN_OUTPUT_BYTES,
            "执行输出超过大小限制"
        );
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_RUN_OUTPUT_BYTES,
            "执行输出超过大小限制"
        );
        Ok(bytes)
    }

    pub fn create_run(&self, run: &Run, task: &Task) -> Result<RunWriter> {
        let directory = self.run_path(&task.id, &run.id)?;
        private_dir(directory.parent().unwrap())?;
        let temporary = directory
            .parent()
            .unwrap()
            .join(format!(".{}.tmp", aow_id::new_id()));
        let result = (|| {
            fs::create_dir(&temporary)?;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
            let events = temporary.join("events.jsonl");
            let file = OpenOptions::new()
                .read(true)
                .append(true)
                .create_new(true)
                .mode(0o600)
                .open(&events)?;
            ensure!(try_exclusive(&file)?, "执行文件已被占用");
            for output in RunOutput::ALL {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(temporary.join(output.filename()))?;
            }
            let mut writer = RunWriter {
                file,
                directory: temporary.clone(),
                active_path: None,
            };
            writer.append(&RunEvent::Started {
                run: Box::new(run.clone()),
                configuration: task.input.clone(),
            })?;
            // Publish the complete artifact directory without overwriting an
            // existing run, then expose its locked event journal as active.
            fs::rename(&temporary, &directory)?;
            writer.directory = directory.clone();
            let active = directory.parent().unwrap().join(".active");
            private_dir(&active)?;
            let active_path = active.join(&run.id);
            fs::hard_link(directory.join("events.jsonl"), &active_path)?;
            writer.active_path = Some(active_path);
            Ok(writer)
        })();
        let _ = fs::remove_dir_all(temporary);
        result
    }
}
