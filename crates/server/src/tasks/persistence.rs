use super::requests::{CreationReceipt, CreationRequest};
use super::store::{TaskData, identifier, text};
use crate::aow::atomic_save_document;
use anyhow::{Context, Result, ensure};
use aow_config::ConfigRepository;
use aow_protocol::{BoardTask, InboxItem, InboxSummary, TaskBoard, TaskStatus};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct Persistence {
    pub config: ConfigRepository,
    pub runtime: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Statuses {
    version: u32,
    pub revision: u64,
    pub statuses: Vec<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<CreationRequest>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeTasks {
    version: u32,
    tasks: Vec<BoardTask>,
    #[serde(default)]
    requests: BTreeMap<String, CreationReceipt>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct StoredInbox {
    #[serde(flatten)]
    pub item: InboxItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation: Option<CreationRequest>,
}

impl Persistence {
    pub fn new(state_dir: &Path, config: ConfigRepository) -> Self {
        let runtime = state_dir
            .join("tasks")
            .join(config.selection().config_id)
            .join("tasks.json");
        Self { config, runtime }
    }

    pub fn load(&self, data: &mut TaskData) -> Result<()> {
        let statuses = self.config.directory().join("tasks/statuses.json");
        if let Some(document) = read_optional::<Statuses>(&statuses)? {
            ensure!(document.version == 1, "Unsupported task status format");
            ensure!(
                !document.statuses.is_empty(),
                "At least one task status is required"
            );
            let mut ids = std::collections::HashSet::new();
            for status in &document.statuses {
                ensure!(
                    identifier(&status.id).is_ok()
                        && text(&status.name, 128).is_ok()
                        && ids.insert(&status.id),
                    "Invalid or duplicate task status"
                );
                ensure!(
                    status.color.len() == 7
                        && status.color.starts_with('#')
                        && status.color[1..].bytes().all(|b| b.is_ascii_hexdigit()),
                    "Invalid task status color"
                );
            }
            data.board.status_revision = document.revision;
            data.board.statuses = document.statuses;
            data.status_request = document.request;
        } else {
            self.save_statuses(&data.board, None)?;
        }
        if let Some(document) = read_optional::<RuntimeTasks>(&self.runtime)? {
            ensure!(document.version == 1, "Unsupported runtime task format");
            let mut ids = std::collections::HashSet::new();
            for task in &document.tasks {
                ensure!(
                    identifier(&task.id).is_ok() && ids.insert(&task.id),
                    "Invalid or duplicate task ID"
                );
            }
            data.board.tasks = document.tasks;
            data.task_requests = document.requests;
        }
        for project in directories(&self.config.directory().join("tasks/inbox"))? {
            ensure!(
                project.file_type()?.is_dir(),
                "Inbox project must be a directory: {}",
                project.path().display()
            );
            let project_id = project.file_name().to_string_lossy().into_owned();
            ensure!(
                identifier(&project_id).is_ok(),
                "Invalid Inbox project ID: {project_id}"
            );
            for entry in directories(&project.path())? {
                if entry.path().extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                ensure!(
                    entry.file_type()?.is_file(),
                    "Requirement must be a regular file: {}",
                    entry.path().display()
                );
                let stored = self.read_inbox(&entry.path())?;
                let item = stored.item;
                if let Some(request) = stored.creation {
                    ensure!(
                        data.inbox_requests
                            .insert(request.key.clone(), request.receipt(&item.id))
                            .is_none(),
                        "Duplicate Inbox creation request"
                    );
                }
                ensure!(
                    item.project_id == project_id
                        && entry.file_name() == format!("{}.json", item.id).as_str(),
                    "Requirement ID/project does not match its path: {}",
                    entry.path().display()
                );
                let id = item.id.clone();
                ensure!(
                    data.inbox
                        .insert(id.clone(), InboxSummary::from(&item))
                        .is_none(),
                    "Duplicate requirement ID: {id}"
                );
            }
        }
        Ok(())
    }

    pub fn read_inbox(&self, path: &Path) -> Result<StoredInbox> {
        let stored: StoredInbox = read_optional(path)?
            .with_context(|| format!("Requirement does not exist: {}", path.display()))?;
        let item = &stored.item;
        ensure!(
            identifier(&item.id).is_ok()
                && identifier(&item.project_id).is_ok()
                && text(&item.title, 512).is_ok()
                && (item.description.is_empty() || text(&item.description, 100_000).is_ok())
                && chrono::DateTime::parse_from_rfc3339(&item.created_at).is_ok()
                && chrono::DateTime::parse_from_rfc3339(&item.updated_at).is_ok(),
            "Invalid requirement: {}",
            path.display()
        );
        Ok(stored)
    }

    pub fn inbox_path(project_id: &str, id: &str) -> PathBuf {
        Path::new("tasks/inbox")
            .join(project_id)
            .join(format!("{id}.json"))
    }

    pub fn save_inbox(&self, item: &InboxItem, creation: Option<&CreationRequest>) -> Result<()> {
        self.save_config(
            &Self::inbox_path(&item.project_id, &item.id),
            &StoredInbox {
                item: item.clone(),
                creation: creation.cloned(),
            },
        )
    }

    pub fn read_statuses(&self) -> Result<Option<Statuses>> {
        read_optional(&self.config.directory().join("tasks/statuses.json"))
    }

    pub fn save_statuses(
        &self,
        board: &TaskBoard,
        request: Option<&CreationRequest>,
    ) -> Result<()> {
        self.save_config(
            Path::new("tasks/statuses.json"),
            &Statuses {
                version: 1,
                revision: board.status_revision,
                statuses: board.statuses.clone(),
                request: request.cloned(),
            },
        )
    }

    pub fn save_tasks(
        &self,
        tasks: &[BoardTask],
        requests: &BTreeMap<String, CreationReceipt>,
    ) -> Result<()> {
        atomic_save_document(
            &self.runtime,
            &RuntimeTasks {
                version: 1,
                tasks: tasks.to_vec(),
                requests: requests.clone(),
            },
        )
        .map_err(Into::into)
    }

    fn save_config(&self, relative: &Path, value: &impl Serialize) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        self.config.save(relative, &bytes)
    }
}

fn read_optional<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.is_file(),
            "Expected a regular file: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("Invalid JSON: {}", path.display()))
        .map(Some)
}

fn directories(path: &Path) -> Result<Vec<fs::DirEntry>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.is_dir(),
            "Expected a directory: {}",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.into()),
    }
    Ok(fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?)
}
