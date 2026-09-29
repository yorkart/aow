use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};

use anyhow::{Context, Result, ensure};

use crate::Task;

use super::{FileLock, Store, private_dir, try_shared, valid_component};

impl Store {
    pub fn task_path(&self, id: &str) -> Result<PathBuf> {
        valid_component(id)?;
        Ok(self
            .config_dir
            .join("automations/tasks")
            .join(format!("{id}.json")))
    }

    /// Allocate a task ID from the shared local Snowflake generator.
    pub fn new_task_id(&self) -> Result<String> {
        Ok(aow_id::new_id())
    }

    pub fn get_task(&self, id: &str) -> Result<Task> {
        let task: Task = serde_json::from_slice(
            &fs::read(self.task_path(id)?).context("自动化任务不存在或无法读取")?,
        )?;
        ensure!(task.id == id, "任务文件 ID 不匹配");
        Ok(task)
    }

    fn manual_request_path(&self, task_id: &str, run_id: &str) -> Result<PathBuf> {
        valid_component(task_id)?;
        valid_component(run_id)?;
        Ok(self
            .root
            .join("pending")
            .join(task_id)
            .join(format!("{run_id}.json")))
    }

    pub fn save_manual_request(
        &self,
        run_id: &str,
        request: &crate::ManualRunRequest,
    ) -> Result<()> {
        let path = self.manual_request_path(&request.task.id, run_id)?;
        private_dir(path.parent().unwrap())?;
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(&serde_json::to_vec(request)?)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(path)?;
        Ok(())
    }

    /// Requests are private, per-run inputs, consumed even if execution is skipped.
    pub fn take_manual_request(
        &self,
        task_id: &str,
        run_id: &str,
    ) -> Result<Option<crate::ManualRunRequest>> {
        let path = self.manual_request_path(task_id, run_id)?;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        fs::remove_file(path)?;
        let request: crate::ManualRunRequest = serde_json::from_slice(&bytes)?;
        ensure!(request.task.id == task_id, "手动执行配置的任务 ID 不匹配");
        Ok(Some(request))
    }
    pub fn save_task(&self, task: &Task) -> Result<()> {
        let path = self.task_path(&task.id)?;
        self.config
            .as_ref()
            .context("请先初始化配置仓库再保存任务")?
            .save(
                path.strip_prefix(&self.config_dir)?,
                &serde_json::to_vec_pretty(task)?,
            )
    }
    pub fn tasks(&self) -> Result<Vec<Task>> {
        Ok(self
            .all_tasks()?
            .into_iter()
            .filter(|task| !task.deleted)
            .collect())
    }

    pub(crate) fn all_tasks(&self) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        let entries = match fs::read_dir(self.config_dir.join("automations/tasks")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(tasks),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|v| v == "json") {
                let task: Task = serde_json::from_slice(&fs::read(&path)?)
                    .with_context(|| format!("任务文件损坏: {}", path.display()))?;
                ensure!(
                    path.file_stem().and_then(|v| v.to_str()) == Some(&task.id),
                    "任务文件 ID 不匹配"
                );
                tasks.push(task);
            }
        }
        tasks.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(tasks)
    }

    pub fn is_running(&self, task_id: &str) -> Result<bool> {
        valid_component(task_id)?;
        Ok(self.has_active_runs(task_id)? || self.has_occupied_concurrency_slot(task_id)?)
    }

    pub(crate) fn has_active_runs(&self, task_id: &str) -> Result<bool> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id).join(".active");
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let file = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(entry?.path())
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let acquired = try_shared(&file)?;
            let _guard = FileLock {
                file,
                owns_lock: acquired,
            };
            if !acquired {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
