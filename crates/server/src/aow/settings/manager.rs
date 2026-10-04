use std::{path::Path, path::PathBuf, sync::MutexGuard};

use aow_agents::environment;

use super::super::{AowError, AowManager};
use super::configuration::{
    UpdateSettingsRequest, normalize_node_addresses, prepare_notes_base_directory,
};
use super::model::AowSettings;

impl AowManager {
    pub(in crate::aow) fn lock_settings(&self) -> Result<MutexGuard<'_, AowSettings>, AowError> {
        self.inner.settings.lock().map_err(|_| AowError::Poisoned)
    }

    pub(in crate::aow) fn settings(&self) -> Result<AowSettings, AowError> {
        Ok(self.lock_settings()?.clone())
    }
    pub(crate) async fn execution_path(&self) -> Result<Vec<PathBuf>, AowError> {
        if let Some(path) = self.settings()?.execution_path {
            return Ok(path);
        }
        let path = environment::normalize_path(super::super::discovery_path().await)
            .map_err(|error| AowError::Invalid(error.to_string()))?;
        let mut settings = self.lock_settings()?;
        // A concurrent Settings update wins over default discovery.
        if let Some(path) = &settings.execution_path {
            return Ok(path.clone());
        }
        settings.execution_path = Some(path.clone());
        if let Err(error) = self.persist_settings(&settings) {
            settings.execution_path = None;
            return Err(error);
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
            Some(prepare_notes_base_directory(Path::new(notes_base.trim())).await?)
        } else {
            None
        };
        if execution_path.is_none() {
            self.execution_path().await?;
        }
        let operation = self.inner.project_operation.clone().lock_owned().await;
        self.update_notes_settings(
            notes_base,
            execution_path,
            request.editor,
            node_addresses,
            operation,
        )
        .await
    }
}
