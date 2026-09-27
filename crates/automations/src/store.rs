use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use aow_config::ConfigRepository;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Run, RunDetail, RunEvent, RunOutput, RunStatus, Task, TaskInput};

const MAX_RUN_EVENTS_BYTES: u64 = 1024 * 1024;
pub const MAX_RUN_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StoredRunEvent {
    Started {
        run: Box<Run>,
        configuration: TaskInput,
    },
    Workspace {
        path: PathBuf,
        #[serde(default)]
        branch: Option<String>,
        elapsed_ms: u64,
    },
    AgentStarted {
        pid: u32,
        #[serde(default)]
        command: Vec<String>,
    },
    Session {
        session_id: String,
        elapsed_ms: u64,
    },
    Finished {
        at: chrono::DateTime<Utc>,
        status: RunStatus,
        exit_code: Option<i32>,
        message: Option<String>,
        duration_ms: u64,
    },
    OutputTruncated {},
}

#[derive(Clone)]
pub struct Store {
    pub state_dir: PathBuf,
    pub root: PathBuf,
    pub config_dir: PathBuf,
    config: Option<ConfigRepository>,
}

pub fn valid_component(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 96
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
        "无效的任务或执行 ID"
    );
    Ok(())
}

fn random_decimal(width: usize) -> String {
    let upper_bound = 10_u128.pow(width as u32);
    format!(
        "{:0width$}",
        Uuid::new_v4().as_u128() % upper_bound,
        width = width
    )
}

pub fn new_run_id() -> String {
    format!(
        "{}_{}",
        Utc::now().format("%Y%m%dT%H%M%S%3fZ"),
        random_decimal(4)
    )
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "数据目录不能是符号链接"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("文件缺少父目录")?;
    if !parent.exists() {
        private_dir(parent)?;
    }
    let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4().as_simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(crate) fn try_exclusive(file: &File) -> Result<bool> {
    try_lock(file, libc::LOCK_EX)
}

fn try_shared(file: &File) -> Result<bool> {
    try_lock(file, libc::LOCK_SH)
}

pub(crate) fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

fn try_lock(file: &File, operation: libc::c_int) -> Result<bool> {
    if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(error.into())
    }
}

impl Store {
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        ConfigRepository::initialize(&state_dir)?;
        let store = Self::open(state_dir)?;
        private_dir(&store.root)?;
        private_dir(&store.root.join("runs"))?;
        Ok(store)
    }

    /// Resolve paths without creating state, changing permissions, or running cleanup.
    pub fn open(state_dir: PathBuf) -> Result<Self> {
        let state_dir = if state_dir.is_absolute() {
            state_dir
        } else {
            std::env::current_dir()?.join(state_dir)
        };
        let root = state_dir.join("automations");
        let config = ConfigRepository::open(&state_dir)?;
        let config_dir = config
            .as_ref()
            .map(|config| config.directory().to_path_buf())
            .unwrap_or_else(|| state_dir.clone());
        Ok(Self {
            state_dir,
            root,
            config_dir,
            config,
        })
    }

    pub fn task_path(&self, id: &str) -> Result<PathBuf> {
        valid_component(id)?;
        Ok(self
            .config_dir
            .join("automations/tasks")
            .join(format!("{id}.json")))
    }

    /// Generates a short task ID that does not already have a persisted task.
    ///
    /// A task ID is used in externally visible worktree paths, branch names, and
    /// native scheduler unit names, so keep it compact while regenerating a
    /// candidate if its task file already exists.
    pub fn new_task_id(&self) -> Result<String> {
        self.new_task_id_with(|| random_decimal(8))
    }

    fn new_task_id_with(&self, mut next_candidate: impl FnMut() -> String) -> Result<String> {
        loop {
            let id = next_candidate();
            let path = self.task_path(&id)?;
            match fs::symlink_metadata(path) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(id),
                Err(error) => return Err(error.into()),
            }
        }
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

    pub fn run_path(&self, task_id: &str, run_id: &str) -> Result<PathBuf> {
        valid_component(task_id)?;
        valid_component(run_id)?;
        Ok(self.root.join("runs").join(task_id).join(run_id))
    }

    fn run_events_path(&self, task_id: &str, run_id: &str) -> Result<PathBuf> {
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
            .join(format!(".{}.tmp", Uuid::new_v4().as_simple()));
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

    pub fn read_run(&self, task_id: &str, run_id: &str) -> Result<Option<Run>> {
        Ok(self
            .read_run_detail(task_id, run_id)?
            .map(|detail| detail.run))
    }

    pub fn read_run_detail(&self, task_id: &str, run_id: &str) -> Result<Option<RunDetail>> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.run_events_path(task_id, run_id)?)?;
        let live = !try_shared(&file)?;
        let mut file = FileLock {
            file,
            owns_lock: !live,
        };
        ensure!(
            file.metadata()?.len() <= MAX_RUN_EVENTS_BYTES,
            "执行记录超过大小限制"
        );
        ensure!(file.metadata()?.is_file(), "执行记录不是普通文件");
        let mut bytes = Vec::new();
        (&mut *file)
            .take(MAX_RUN_EVENTS_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_RUN_EVENTS_BYTES,
            "执行记录超过大小限制"
        );
        let mut run = None;
        let mut configuration = None;
        for line in bytes.split_inclusive(|v| *v == b'\n') {
            // A writer may still be appending, or may have died mid-record.
            if !line.ends_with(b"\n") {
                break;
            }
            let event: StoredRunEvent =
                serde_json::from_slice(line).context("执行记录包含无效 JSON")?;
            if let StoredRunEvent::Started {
                run: started,
                configuration: input,
            } = event
            {
                ensure!(run.is_none(), "执行记录包含重复开始事件");
                run = Some(*started);
                configuration = Some(input);
                continue;
            }
            let Some(run) = run.as_mut() else {
                continue;
            };
            match event {
                StoredRunEvent::Workspace {
                    path,
                    branch,
                    elapsed_ms,
                } => {
                    run.workspace_path = Some(path);
                    run.branch = branch;
                    run.preparation_ms = Some(elapsed_ms);
                }
                StoredRunEvent::AgentStarted { pid, command } => {
                    run.agent_pid = Some(pid);
                    run.agent_command = Some(command);
                    run.status = RunStatus::Running;
                }
                StoredRunEvent::Session {
                    session_id,
                    elapsed_ms,
                } => {
                    run.session_id = Some(session_id);
                    run.session_acquired_ms = Some(elapsed_ms);
                }
                StoredRunEvent::Finished {
                    at,
                    status,
                    exit_code,
                    message,
                    duration_ms,
                } => {
                    run.finished_at = Some(at);
                    run.status = status;
                    run.exit_code = exit_code;
                    run.message = message;
                    run.duration_ms = Some(duration_ms);
                }
                StoredRunEvent::OutputTruncated {} => {}
                StoredRunEvent::Started { .. } => unreachable!(),
            }
        }
        if let Some(run) = run.as_mut() {
            ensure!(
                run.id == run_id && run.task_id == task_id,
                "执行文件 ID 不匹配"
            );
            if !run.status.terminal() && !live {
                run.status = RunStatus::Interrupted;
                run.message = Some("执行进程已结束，未留下完成记录".into());
            }
        }
        Ok(run.map(|run| RunDetail {
            run,
            configuration: configuration.expect("started event contains configuration"),
            stdout_path: self
                .run_output_path(task_id, run_id, RunOutput::Stdio)
                .expect("validated IDs"),
            stderr_path: self
                .run_output_path(task_id, run_id, RunOutput::Stderr)
                .expect("validated IDs"),
        }))
    }

    pub fn runs(&self, task_id: &str, before: Option<&str>, limit: usize) -> Result<Vec<Run>> {
        let mut runs = Vec::new();
        if limit == 0 {
            return Ok(runs);
        }
        for id in self.run_ids(task_id, before)? {
            match self.read_run(task_id, &id) {
                Ok(Some(run)) => runs.push(run),
                Ok(None) => continue,
                Err(error) if is_not_found(&error) => continue,
                Err(error) => return Err(error),
            }
            if runs.len() >= limit.min(500) {
                break;
            }
        }
        Ok(runs)
    }

    pub(crate) fn run_ids(&self, task_id: &str, before: Option<&str>) -> Result<Vec<String>> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id);
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry?;
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if file_type.is_dir() && !file_type.is_symlink() {
                let id = entry.file_name().to_string_lossy().into_owned();
                if valid_component(&id).is_ok() && before.is_none_or(|before| id.as_str() < before)
                {
                    ids.push(id);
                }
            }
        }
        ids.sort_unstable_by(|a, b| b.cmp(a));
        Ok(ids)
    }

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
        run_ids.sort_unstable_by(|a, b| b.cmp(a));

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

pub struct RunWriter {
    file: File,
    directory: PathBuf,
    active_path: Option<PathBuf>,
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

/// Release advisory locks explicitly: another thread can fork between acquiring
/// a lock and dropping its owner, briefly inheriting the open file description.
struct FileLock {
    file: File,
    owns_lock: bool,
}
impl Drop for FileLock {
    fn drop(&mut self) {
        if self.owns_lock {
            unsafe {
                libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}
impl std::ops::Deref for FileLock {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl std::ops::DerefMut for FileLock {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_retries_when_the_candidate_already_exists() {
        let temporary = tempfile::tempdir().unwrap();
        let store = Store::new(temporary.path().join("state")).unwrap();
        private_dir(&store.config_dir.join("automations/tasks")).unwrap();
        fs::write(store.task_path("12345678").unwrap(), b"{}").unwrap();
        let mut candidates = ["12345678", "87654321"].into_iter();

        let id = store
            .new_task_id_with(|| candidates.next().unwrap().to_owned())
            .unwrap();

        assert_eq!(id, "87654321");
    }

    #[test]
    fn run_id_has_a_timestamp_and_four_digit_random_suffix() {
        let id = new_run_id();
        let (timestamp, suffix) = id.rsplit_once('_').unwrap();
        assert_eq!(timestamp.len(), 19);
        assert_eq!(suffix.len(), 4);
        assert!(timestamp.ends_with('Z'));
        assert!(suffix.bytes().all(|byte| byte.is_ascii_digit()));
    }
}
