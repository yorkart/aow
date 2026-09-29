use super::{persistence::Persistence, requests::CreationRequest, store::*};
use crate::HttpError;
use aow_protocol::*;
use std::collections::HashMap;

impl TaskStore {
    pub(super) fn get_inbox(&self, id: &str) -> Result<InboxItem, HttpError> {
        self.read_inbox(&*self.lock()?, id)
    }

    fn read_inbox(&self, data: &TaskData, id: &str) -> Result<InboxItem, HttpError> {
        let summary = data.inbox.get(id).ok_or_else(missing)?;
        if let Some(persistence) = &self.persistence {
            let path = persistence
                .config
                .directory()
                .join(Persistence::inbox_path(&summary.project_id, id));
            let item = persistence
                .read_inbox(&path)
                .map_err(|e| HttpError::internal(format!("{e:#}")))?
                .item;
            if item.id != id || item.project_id != summary.project_id {
                return Err(conflict("Requirement identity changed on disk"));
            }
            Ok(item)
        } else {
            data.memory_inbox.get(id).cloned().ok_or_else(missing)
        }
    }

    pub(super) fn write_inbox(&self, input: InboxWrite) -> Result<InboxItem, HttpError> {
        self.write_inbox_with_request(input, None)
    }

    pub(super) fn create_inbox(&self, input: InboxCreate) -> Result<InboxItem, HttpError> {
        let request = CreationRequest::new(&input.request_key, &input)?;
        self.write_inbox_with_request(
            InboxWrite {
                id: String::new(),
                project_id: input.project_id,
                title: input.title,
                description: input.description,
                expected_revision: None,
            },
            Some(request),
        )
    }

    fn write_inbox_with_request(
        &self,
        mut input: InboxWrite,
        request: Option<CreationRequest>,
    ) -> Result<InboxItem, HttpError> {
        identifier(&input.project_id)?;
        text(&input.title, 512)?;
        if !input.description.is_empty() {
            text(&input.description, 100_000)?;
        }
        let mut data = self.lock()?;
        if let Some(request) = &request {
            input.id = if let Some(receipt) = data.inbox_requests.get(&request.key) {
                request.verify(&receipt.fingerprint)?;
                // Replay returns the current resource without undoing later edits.
                let item = self.read_inbox(&data, &receipt.id)?;
                input.title = item.title;
                input.description = item.description;
                receipt.id.clone()
            } else {
                aow_id::new_id()
            };
        }
        identifier(&input.id)?;
        let creation = request.or_else(|| {
            data.inbox_requests.iter().find_map(|(key, receipt)| {
                (receipt.id == input.id).then(|| CreationRequest {
                    key: key.clone(),
                    fingerprint: receipt.fingerprint.clone(),
                })
            })
        });
        let item = if data.inbox.contains_key(&input.id) {
            let mut item = self.read_inbox(&data, &input.id)?;
            if item.project_id != input.project_id {
                return Err(conflict("This requirement belongs to a different project"));
            }
            let unchanged =
                item.title == input.title.trim() && item.description == input.description;
            // A repeated creation is idempotent. A repeated edit also permits
            // retrying a save whose bytes were written but Git commit failed.
            if !unchanged || input.expected_revision.is_some() {
                let expected = input
                    .expected_revision
                    .ok_or_else(|| conflict("Inbox ID already exists"))?;
                if !(unchanged && expected.checked_add(1) == Some(item.revision)) {
                    revision(item.revision, expected)?;
                }
            }
            if !unchanged {
                item.title = input.title.trim().into();
                item.description = input.description;
                item.revision += 1;
                item.updated_at = now();
            }
            item
        } else {
            if input.expected_revision.is_some() {
                return Err(missing());
            }
            InboxItem {
                id: input.id,
                project_id: input.project_id,
                revision: 1,
                title: input.title.trim().into(),
                description: input.description,
                created_at: now(),
                updated_at: now(),
            }
        };
        if let Some(persistence) = &self.persistence {
            if let Err(error) = persistence.save_inbox(&item, creation.as_ref()) {
                // ConfigRepository deliberately retains a successfully written
                // file when Git fails. Keep the summary consistent and allow retry.
                let path = persistence
                    .config
                    .directory()
                    .join(Persistence::inbox_path(&item.project_id, &item.id));
                if let Ok(saved) = persistence.read_inbox(&path) {
                    if let Some(request) = saved.creation {
                        data.inbox_requests
                            .insert(request.key.clone(), request.receipt(&saved.item.id));
                    }
                    data.inbox
                        .insert(saved.item.id.clone(), InboxSummary::from(&saved.item));
                }
                drop(data);
                self.events.tasks_changed();
                return Err(HttpError::internal(format!("{error:#}")));
            }
        } else {
            data.memory_inbox.insert(item.id.clone(), item.clone());
        }
        data.inbox
            .insert(item.id.clone(), InboxSummary::from(&item));
        if let Some(request) = creation {
            data.inbox_requests
                .insert(request.key.clone(), request.receipt(&item.id));
        }
        drop(data);
        self.events.tasks_changed();
        Ok(item)
    }

    pub(super) fn delete_inbox(&self, id: &str, expected: u64) -> Result<(), HttpError> {
        let mut data = self.lock()?;
        let item = data.inbox.get(id).ok_or_else(missing)?;
        revision(item.revision, expected)?;
        if data.board.tasks.iter().any(|task| task.inbox_id == id) {
            return Err(conflict(
                "Converted requirements are retained as task sources",
            ));
        }
        if let Some(persistence) = &self.persistence {
            persistence
                .config
                .remove(&Persistence::inbox_path(&item.project_id, id))
                .map_err(|e| HttpError::internal(format!("{e:#}")))?;
        }
        data.inbox.remove(id);
        data.memory_inbox.remove(id);
        drop(data);
        self.events.tasks_changed();
        Ok(())
    }

    pub(super) fn inbox_page(
        &self,
        project: Option<&str>,
        include_converted: bool,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<InboxPage, HttpError> {
        if !(1..=200).contains(&limit) {
            return Err(invalid("Inbox limit must be between 1 and 200"));
        }
        let cursor = cursor
            .map(|cursor| {
                let (time, id) = cursor
                    .split_once('_')
                    .ok_or_else(|| invalid("Invalid Inbox cursor"))?;
                identifier(id)?;
                Ok::<_, HttpError>((
                    time.parse::<i64>()
                        .map_err(|_| invalid("Invalid Inbox cursor"))?,
                    id,
                ))
            })
            .transpose()?;
        let data = self.lock()?;
        let mut linked: HashMap<&str, Vec<String>> = HashMap::new();
        for task in &data.board.tasks {
            linked
                .entry(&task.inbox_id)
                .or_default()
                .push(task.id.clone());
        }
        let mut items: Vec<_> = data
            .inbox
            .values()
            .filter(|item| project.is_none_or(|id| item.project_id == id))
            .filter(|item| include_converted || !linked.contains_key(item.id.as_str()))
            .map(|item| {
                (
                    chrono::DateTime::parse_from_rfc3339(&item.created_at)
                        .unwrap()
                        .timestamp_micros(),
                    item,
                )
            })
            .collect();
        items.sort_unstable_by(|a, b| (b.0, &b.1.id).cmp(&(a.0, &a.1.id)));
        let total = items.len();
        let mut page = items
            .into_iter()
            .filter(|(time, item)| cursor.is_none_or(|cursor| (*time, item.id.as_str()) < cursor));
        let items: Vec<_> = page
            .by_ref()
            .take(limit)
            .map(|(_, item)| {
                let mut item = item.clone();
                item.task_ids = linked.get(item.id.as_str()).cloned().unwrap_or_default();
                item
            })
            .collect();
        let next_cursor = if page.next().is_some() {
            items.last().map(|item| {
                format!(
                    "{}_{}",
                    chrono::DateTime::parse_from_rfc3339(&item.created_at)
                        .unwrap()
                        .timestamp_micros(),
                    item.id
                )
            })
        } else {
            None
        };
        Ok(InboxPage {
            items,
            total,
            next_cursor,
        })
    }

    pub(super) fn create_task(
        &self,
        task: BoardTask,
        expected_inbox_revision: u64,
        request: Option<CreationRequest>,
    ) -> Result<(BoardTask, bool), HttpError> {
        let mut data = self.lock()?;
        if let Some(request) = &request
            && let Some(receipt) = data.task_requests.get(&request.key)
        {
            request.verify(&receipt.fingerprint)?;
            let task = data
                .board
                .tasks
                .iter()
                .find(|task| task.id == receipt.id)
                .cloned()
                .ok_or_else(missing)?;
            return Ok((task, false));
        }
        if data
            .board
            .tasks
            .iter()
            .any(|existing| existing.id == task.id)
        {
            return Err(conflict("Conversion already accepted"));
        }
        if !data
            .board
            .statuses
            .iter()
            .any(|status| status.id == task.status_id)
        {
            return Err(invalid("Unknown task status"));
        }
        let item = self.read_inbox(&data, &task.inbox_id)?;
        revision(item.revision, expected_inbox_revision)?;
        if item.project_id != task.project_id {
            return Err(conflict("This requirement belongs to a different project"));
        }
        let mut next = data.board.clone();
        next.tasks.insert(0, task.clone());
        let mut requests = data.task_requests.clone();
        if let Some(request) = request {
            requests.insert(request.key.clone(), request.receipt(&task.id));
        }
        if let Some(persistence) = &self.persistence {
            persistence
                .save_tasks(&next.tasks, &requests)
                .map_err(|e| HttpError::internal(format!("{e:#}")))?;
        }
        data.task_requests = requests;
        data.board = next;
        drop(data);
        self.events.tasks_changed();
        Ok((task, true))
    }

    pub(super) fn task_for_request(
        &self,
        request: &CreationRequest,
    ) -> Result<Option<BoardTask>, HttpError> {
        let data = self.lock()?;
        let Some(receipt) = data.task_requests.get(&request.key) else {
            return Ok(None);
        };
        request.verify(&receipt.fingerprint)?;
        Ok(Some(
            data.board
                .tasks
                .iter()
                .find(|task| task.id == receipt.id)
                .cloned()
                .ok_or_else(missing)?,
        ))
    }
}
