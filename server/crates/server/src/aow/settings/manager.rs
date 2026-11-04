use std::{path::Path, path::PathBuf, sync::MutexGuard};

use aow_agents::environment;

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
        if let Some(runtime) = &self.inner.runtime_client {
            let value: serde_json::Value = runtime
                .get_json("/v1/environment")
                .await
                .map_err(|error| AowError::Invalid(error.to_string()))?;
            return serde_json::from_value(value["execution_path"].clone()).map_err(Into::into);
        }
        #[cfg(test)]
        {
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
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Runtime is unavailable".into()))
        }
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
        if let (Some(runtime), Some(paths)) = (&self.inner.runtime_client, &execution_path) {
            let _: serde_json::Value = runtime
                .put_json(
                    "/v1/environment",
                    &serde_json::json!({"execution_path":paths}),
                )
                .await
                .map_err(|error| AowError::Invalid(error.to_string()))?;
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
