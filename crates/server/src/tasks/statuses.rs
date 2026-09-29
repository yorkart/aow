use super::{requests::CreationRequest, store::*};
use crate::HttpError;
use aow_protocol::{TaskBoard, TaskStatus, TaskStatusesWrite};
use std::collections::HashSet;

impl TaskStore {
    pub(super) fn write_statuses(&self, input: TaskStatusesWrite) -> Result<TaskBoard, HttpError> {
        let request = CreationRequest::new(&input.request_key, &input)?;
        if input.statuses.is_empty() {
            return Err(invalid("The board needs at least one status"));
        }
        let mut data = self.lock()?;
        if let Some(previous) = &data.status_request
            && previous.key == request.key
        {
            request.verify(&previous.fingerprint)?;
            // Also retry the Git commit after a successful file write.
            if let Some(persistence) = &self.persistence {
                persistence
                    .save_statuses(&data.board, Some(previous))
                    .map_err(|e| HttpError::internal(format!("{e:#}")))?;
            }
            return Ok(data.board.clone());
        }
        revision(data.board.status_revision, input.expected_revision)?;
        let mut ids = HashSet::new();
        let mut statuses = Vec::new();
        for status in input.statuses {
            text(&status.name, 128)?;
            if status.color.len() != 7
                || !status.color.starts_with('#')
                || !status.color[1..].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(invalid("Use a six-digit hex color"));
            }
            let id = match status.id {
                Some(id) => {
                    identifier(&id)?;
                    if !data.board.statuses.iter().any(|existing| existing.id == id) {
                        return Err(invalid("Unknown task status"));
                    }
                    id
                }
                None => aow_id::new_id(),
            };
            if !ids.insert(id.clone()) {
                return Err(invalid("Status IDs must be unique"));
            }
            statuses.push(TaskStatus {
                id,
                name: status.name,
                color: status.color,
            });
        }
        if data
            .board
            .tasks
            .iter()
            .any(|task| !ids.contains(&task.status_id))
        {
            return Err(conflict(
                "Cannot remove a status used by a task. Move its tasks first.",
            ));
        }
        let mut next = data.board.clone();
        next.statuses = statuses;
        next.status_revision += 1;
        if let Some(persistence) = &self.persistence
            && let Err(error) = persistence.save_statuses(&next, Some(&request))
        {
            // Config writes may reach disk before their Git commit fails. Keep
            // the assigned IDs and receipt so retrying cannot allocate replacements.
            if let Ok(Some(saved)) = persistence.read_statuses()
                && saved.revision == next.status_revision
                && saved.statuses == next.statuses
                && saved
                    .request
                    .as_ref()
                    .is_some_and(|r| r.key == request.key && r.fingerprint == request.fingerprint)
            {
                data.board = next;
                data.status_request = Some(request);
                drop(data);
                self.events.tasks_changed();
            }
            return Err(HttpError::internal(format!("{error:#}")));
        }
        data.board = next.clone();
        data.status_request = Some(request);
        drop(data);
        self.events.tasks_changed();
        Ok(next)
    }
}
