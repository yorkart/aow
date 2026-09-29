use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read},
    os::unix::fs::OpenOptionsExt,
};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::super::{Store, is_not_found, valid_component};
use super::RunCursor;
use super::read::MAX_RUN_EVENTS_BYTES;

#[derive(Deserialize)]
struct StartedEvent {
    #[serde(rename = "type")]
    kind: String,
    run: RunIdentity,
}

#[derive(Deserialize)]
struct RunIdentity {
    id: String,
    task_id: String,
    started_at: DateTime<Utc>,
}

impl Store {
    /// Pagination and retention share the recorded start time. IDs only break
    /// ties; their spelling and generating algorithm carry no time semantics.
    pub(crate) fn run_ids(&self, task_id: &str, before: Option<&str>) -> Result<Vec<String>> {
        self.run_ids_with_errors(task_id, before, |_, error| Err(error))
    }

    pub(super) fn run_ids_with_errors(
        &self,
        task_id: &str,
        before: Option<&str>,
        mut on_error: impl FnMut(&str, anyhow::Error) -> Result<()>,
    ) -> Result<Vec<String>> {
        valid_component(task_id)?;
        let before = before.map(str::parse::<RunCursor>).transpose()?;
        let directory = self.root.join("runs").join(task_id);
        let entries = match fs::read_dir(directory) {
            Ok(entries) => Some(entries),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let mut runs = Vec::new();
        for entry in entries.into_iter().flatten() {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    on_error(&id, error.into())?;
                    continue;
                }
            };
            if !file_type.is_dir() || file_type.is_symlink() || valid_component(&id).is_err() {
                continue;
            }
            match self.run_started_at(task_id, &id) {
                Ok(Some(started_at)) => runs.push((started_at, id)),
                Ok(None) => continue,
                Err(error) if is_not_found(&error) => continue,
                Err(error) => on_error(&id, error)?,
            }
        }
        runs.sort_unstable_by(|left, right| right.cmp(left));
        if let Some(before) = before {
            runs.retain(|(time, id)| {
                (*time, id.as_str()) < (before.started_at, before.id.as_str())
            });
        }
        Ok(runs.into_iter().map(|(_, id)| id).collect())
    }

    fn run_started_at(&self, task_id: &str, run_id: &str) -> Result<Option<DateTime<Utc>>> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.run_events_path(task_id, run_id)?)?;
        ensure!(file.metadata()?.is_file(), "执行记录不是普通文件");
        // The complete start event is published before writers append to the
        // journal. Read just this immutable header when ordering the history.
        let mut line = Vec::new();
        BufReader::new(file.take(MAX_RUN_EVENTS_BYTES + 1)).read_until(b'\n', &mut line)?;
        ensure!(
            line.len() as u64 <= MAX_RUN_EVENTS_BYTES,
            "执行记录超过大小限制"
        );
        if !line.ends_with(b"\n") {
            return Ok(None);
        }
        let event: StartedEvent =
            serde_json::from_slice(&line).context("执行记录包含无效开始事件")?;
        ensure!(event.kind == "started", "执行记录缺少开始事件");
        ensure!(
            event.run.id == run_id && event.run.task_id == task_id,
            "执行文件 ID 不匹配"
        );
        Ok(Some(event.run.started_at))
    }
}
