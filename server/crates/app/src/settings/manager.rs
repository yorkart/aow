use std::{path::Path, path::PathBuf, sync::MutexGuard};

use aow_config::environment;

use super::configuration::{UpdateSettingsRequest, normalize_node_addresses};
use super::model::AowSettings;
use crate::error::ServiceError;
use crate::settings::SettingsService;

impl SettingsService {
    pub(super) fn lock_settings(&self) -> Result<MutexGuard<'_, AowSettings>, ServiceError> {
        self.inner
            .settings
            .lock()
            .map_err(|_| ServiceError::Poisoned)
    }

    pub(crate) fn settings(&self) -> Result<AowSettings, ServiceError> {
        Ok(self.lock_settings()?.clone())
    }
    pub(crate) async fn execution_path(&self) -> Result<Vec<PathBuf>, ServiceError> {
        let runtime = self.node.agent_node().await?;
        let path = runtime
            .execution_path()
            .await
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        #[cfg(test)]
        if self.node.node().is_none() {
            let mut settings = self.lock_settings()?;
            if settings.execution_path.is_none() {
                settings.execution_path = Some(path.clone());
                self.persist_settings(&settings)?;
            }
        }
        Ok(path)
    }

    pub(crate) async fn update_settings(
        &self,
        request: UpdateSettingsRequest,
    ) -> Result<AowSettings, ServiceError> {
        let node_addresses = request
            .node_addresses
            .map(normalize_node_addresses)
            .transpose()?;
        let execution_path = request
            .execution_path
            .map(environment::normalize_path)
            .transpose()
            .map_err(|error| ServiceError::Invalid(error.to_string()))?;
        let notes_base = if let Some(notes_base) = request.notes_base {
            Some(PathBuf::from(
                self.node
                    .prepare_notes_directory(Path::new(notes_base.trim()))
                    .await?,
            ))
        } else {
            None
        };
        if execution_path.is_none() {
            self.execution_path().await?;
        }
        if let Some(paths) = &execution_path {
            self.node
                .agent_node()
                .await?
                .set_execution_path(paths)
                .await
                .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        }
        let operation = self.inner.operation.clone().lock_owned().await;
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
        .map_err(|error| ServiceError::Invalid(error.to_string()))?
    }
}
