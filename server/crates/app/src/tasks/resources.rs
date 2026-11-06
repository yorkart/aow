use super::*;
impl TaskService {
    /// Recheck under task_resource_access and retain that guard through deletion.
    pub(crate) fn ensure_conversation_deletable(&self, thread: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.inner.tasks.list()?.iter().any(|task| {
                !task.archived && task.initial_thread_id.as_deref() == Some(thread)
            }),
            "Archive or delete the owning task before deleting its main conversation"
        );
        Ok(())
    }

    /// Workspace cleanup deletes history too, so it must respect the same protection.
    pub(crate) async fn ensure_workspace_deletable(
        &self,
        service: &aow_host_client::acp::AcpClient,
        workspace: &str,
    ) -> anyhow::Result<()> {
        let protected = self
            .inner
            .tasks
            .list()?
            .into_iter()
            .filter(|task| !task.archived)
            .filter_map(|task| task.initial_thread_id)
            .collect::<std::collections::HashSet<_>>();
        anyhow::ensure!(
            !service
                .sessions(Some(workspace))
                .await?
                .iter()
                .any(|session| protected.contains(&session.thread_id)),
            "Archive or delete tasks with main conversations in this workspace before cleanup"
        );
        Ok(())
    }

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
