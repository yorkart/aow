use super::*;
impl TaskService {
    #[cfg(test)]
    pub(crate) fn test_db(&self) -> aow_db::tasks::TaskDb {
        self.inner.tasks.clone()
    }

    pub(crate) async fn task_resource_access(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.task_operation.lock().await
    }

    pub(crate) async fn task_scope(
        &self,
        headers: &axum::http::HeaderMap,
        workspace: &Path,
    ) -> Result<Option<String>, HttpError> {
        let Some(value) = headers.get("x-aow-task") else {
            return Ok(None);
        };
        let id = value
            .to_str()
            .map_err(|_| crate::instances::invalid("Invalid task ID"))?;
        let task = self
            .inner
            .tasks
            .list()
            .map_err(model::storage)?
            .into_iter()
            .find(|task| task.id == id && !task.archived)
            .ok_or_else(|| crate::instances::invalid("Active task not found"))?;
        let workspace = self
            .node
            .canonical_directory(workspace)
            .await
            .map_err(|error| crate::instances::invalid(error.to_string()))?;
        let registered = self
            .inner
            .tasks
            .owns_workspace(&task.id, &workspace.to_string_lossy())
            .map_err(model::storage)?;
        if workspace != Path::new(&task.workspace) && !registered {
            return Err(crate::instances::invalid(
                "Resource workspace does not match the task",
            ));
        }
        Ok(Some(id.to_owned()))
    }

    pub(crate) fn attach_task_resource(
        &self,
        task: Option<&str>,
        kind: &str,
        id: &str,
    ) -> Result<(), HttpError> {
        if let Some(task) = task {
            self.inner
                .tasks
                .attach(
                    task,
                    &TaskResource {
                        kind: kind.into(),
                        id: id.into(),
                    },
                )
                .map_err(model::storage)?;
        }
        Ok(())
    }
}
