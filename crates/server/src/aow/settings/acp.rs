//! Settings owns the original JSONC text; the ACP implementation only receives a parsed view.
use super::super::AowManager;
use anyhow::{Context, Result, ensure};
use serde::Serialize;

const EMPTY: &str =
    "// ACP settings. Comments and trailing commas are supported.\n{\n  \"agent_servers\": {}\n}\n";

#[derive(Serialize)]
pub(crate) struct AcpSettingsFile {
    pub path: String,
    pub content: String,
    pub revision: String,
}

impl AowManager {
    pub(crate) fn acp_settings(&self) -> Result<AcpSettingsFile> {
        let Some(repository) = &self.inner.config else {
            return Ok(AcpSettingsFile {
                path: aow_acp::SETTINGS_PATH.into(),
                content: EMPTY.into(),
                revision: format!("{:x}", md5::compute(EMPTY)),
            });
        };
        let path = repository.directory().join(aow_acp::SETTINGS_PATH);
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                repository.save(
                    std::path::Path::new(aow_acp::SETTINGS_PATH),
                    EMPTY.as_bytes(),
                )?;
                EMPTY.into()
            }
            Err(error) => return Err(error.into()),
        };
        Ok(AcpSettingsFile {
            path: path.to_string_lossy().into_owned(),
            revision: format!("{:x}", md5::compute(&content)),
            content,
        })
    }
    pub(crate) async fn save_acp_settings(
        &self,
        content: String,
        revision: String,
    ) -> Result<AcpSettingsFile> {
        aow_acp::validate_settings(&content)?;
        let operation = self.inner.project_operation.clone().lock_owned().await;
        let manager = self.clone();
        aow_i18n::spawn_blocking(move || {
            let _operation = operation;
            let previous = manager.acp_settings()?;
            ensure!(
                previous.revision == revision,
                aow_i18n::message(
                    "server.acp_configuration_changed_in_another_window_or_program_reload",
                    &[]
                )
            );
            manager
                .inner
                .config
                .as_ref()
                .context(aow_i18n::text(
                    "server.no_persistent_directory_configured_for_this_service",
                ))?
                .save(
                    std::path::Path::new(aow_acp::SETTINGS_PATH),
                    content.as_bytes(),
                )?;
            manager.acp_settings()
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn jsonc_has_a_separate_file_and_preserves_unknown_fields() {
        let temporary = tempfile::tempdir().unwrap();
        let manager = AowManager::persistent_with_notes_base(
            temporary.path(),
            temporary.path().join("notes"),
        )
        .unwrap();
        let initial = manager.acp_settings().unwrap();
        assert!(initial.path.ends_with("/aow-acp.json"));
        let content = "// retain this comment\n{\"agent_servers\":{\"demo\":{\"type\":\"custom\",\"command\":\"node\",\"future\":{\"unknown\":true},},},\"future_root\":42,}\n";
        let saved = manager
            .save_acp_settings(content.into(), initial.revision.clone())
            .await
            .unwrap();
        assert_eq!(saved.content, content);
        assert_eq!(std::fs::read_to_string(&saved.path).unwrap(), content);
        assert!(
            manager
                .save_acp_settings("{}".into(), initial.revision)
                .await
                .is_err()
        );
        assert!(
            manager
                .save_acp_settings("{bad".into(), saved.revision)
                .await
                .is_err()
        );
        assert_eq!(manager.acp_settings().unwrap().content, content);
        let settings = manager.inner.settings_path.as_ref().unwrap();
        if settings.exists() {
            assert!(
                !std::fs::read_to_string(settings)
                    .unwrap()
                    .contains("agent_servers")
            );
        }
        assert!(!temporary.path().join(".git").exists());
    }
}
