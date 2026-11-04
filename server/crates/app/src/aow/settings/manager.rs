use std::{path::Path, path::PathBuf, sync::MutexGuard};

use aow_config::environment;

use super::super::{AowError, AowManager};
use super::configuration::{UpdateSettingsRequest, normalize_node_addresses};
use super::model::AowSettings;

impl AowManager {
    pub(in crate::aow) fn lock_settings(&self) -> Result<MutexGuard<'_, AowSettings>, AowError> {
        self.inner.settings.lock().map_err(|_| AowError::Poisoned)
    }

    pub(in crate::aow) fn settings(&self) -> Result<AowSettings, AowError> {
        Ok(self.lock_settings()?.clone())
    }
    pub(crate) async fn execution_path(&self) -> Result<Vec<PathBuf>, AowError> {
        let runtime = self.agent_node().await?;
        let path = runtime
            .execution_path()
            .await
            .map_err(|e| AowError::Invalid(e.to_string()))?;
        #[cfg(test)]
        if self.inner.runtime_client.is_none() {
            let mut settings = self.lock_settings()?;
            if settings.execution_path.is_none() {
                settings.execution_path = Some(path.clone());
                self.persist_settings(&settings)?;
            }
        }
        Ok(path)
    }

    pub(in crate::aow) async fn update_settings(
        &self,
        request: UpdateSettingsRequest,
    ) -> Result<AowSettings, AowError> {
        let node_addresses = request
            .node_addresses
            .map(normalize_node_addresses)
            .transpose()?;
        let execution_path = request
            .execution_path
            .map(environment::normalize_path)
            .transpose()
            .map_err(|error| AowError::Invalid(error.to_string()))?;
        let notes_base = if let Some(notes_base) = request.notes_base {
            Some(PathBuf::from(
                self.prepare_notes_directory(Path::new(notes_base.trim()))
                    .await?,
            ))
        } else {
            None
        };
        if execution_path.is_none() {
            self.execution_path().await?;
        }
        if let Some(paths) = &execution_path {
            self.agent_node()
                .await?
                .set_execution_path(paths)
                .await
                .map_err(|e| AowError::Invalid(e.to_string()))?;
        }
        let operation = self.inner.project_operation.clone().lock_owned().await;
        let manager = self.clone();
        aow_i18n::spawn_blocking(move || {
            // Finish persistence even if the HTTP request is cancelled.
            let _operation = operation;
            let mut settings = manager.lock_settings()?;
            let mut next = settings.clone();
            if let Some(base) = notes_base {
                next.notes_base = base.to_string_lossy().into_owned();
            }
            if let Some(path) = execution_path {
                next.execution_path = Some(path);
            }
            if let Some(editor) = request.editor {
                next.editor = editor;
            }
            if let Some(addresses) = node_addresses {
                next.node_addresses = addresses;
            }
            // The root is a default for new projects; existing bindings stay unchanged.
            manager.persist_settings(&next)?;
            *settings = next.clone();
            Ok(next)
        })
        .await
        .map_err(|error| AowError::Invalid(error.to_string()))?
    }
}
