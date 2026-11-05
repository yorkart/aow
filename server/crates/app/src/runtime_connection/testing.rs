//! Settings owns the original JSONC text; the ACP implementation only receives a parsed view.
#[cfg(test)]
use crate::runtime_connection::RuntimeConnection;
use crate::settings::AcpSettingsFile;
#[cfg(test)]
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;

#[cfg(test)]
const EMPTY: &str =
    "// ACP settings. Comments and trailing commas are supported.\n{\n  \"agent_servers\": {}\n}\n";

#[cfg(test)]
impl RuntimeConnection {
    pub(crate) fn set_test_execution_path(&self, paths: Vec<std::path::PathBuf>) {
        self.inner
            .execution_settings
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .execution_path = Some(paths);
    }

    pub(crate) fn bind_test_settings(
        &self,
        settings: std::sync::Arc<std::sync::Mutex<crate::settings::AowSettings>>,
    ) {
        *self.inner.execution_settings.lock().unwrap() = Some(settings);
    }

    pub(crate) fn acp_settings(&self) -> Result<AcpSettingsFile> {
        let Some(repository) = &self.inner.config else {
            return Ok(AcpSettingsFile {
                path: "aow-acp.json".into(),
                content: EMPTY.into(),
                revision: format!("{:x}", md5::compute(EMPTY)),
            });
        };
        let path = repository.directory().join("aow-acp.json");
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                repository.save(std::path::Path::new("aow-acp.json"), EMPTY.as_bytes())?;
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
        let client = self.test_acp_client()?;
        let remote = client.settings().await?;
        client
            .save_settings(content.clone(), remote.revision)
            .await?;
        let operation = self.inner.operation.clone().lock_owned().await;
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
                .save(std::path::Path::new("aow-acp.json"), content.as_bytes())?;
            manager.acp_settings()
        })
        .await?
    }
}

#[cfg(test)]
struct Configuration(RuntimeConnection);
#[cfg(test)]
#[async_trait::async_trait]
impl aow_runtime_client::testing::AcpFixture for Configuration {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(self.0.acp_settings()?.content)
    }
    async fn execution_path(&self) -> anyhow::Result<Option<std::ffi::OsString>> {
        Ok(Some(
            match self
                .0
                .inner
                .execution_settings
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|settings| settings.lock().unwrap().execution_path.clone())
            {
                Some(paths) => std::env::join_paths(paths)?,
                None => std::env::var_os("PATH").unwrap_or_default(),
            },
        ))
    }
}
#[cfg(test)]
impl RuntimeConnection {
    pub(crate) fn test_acp_client(&self) -> anyhow::Result<aow_runtime_client::acp::AcpClient> {
        let client = aow_runtime_client::acp::AcpClient::for_test_host(
            std::sync::Arc::new(Configuration(self.clone())),
            self.inner
                .state_dir
                .as_ref()
                .map(|root| root.join("test-runtime"))
                .or_else(|| Some(self.inner.test_node_root.path().into())),
        )?;
        *self.inner.test_runtime.lock().unwrap() = Some(client.test_runtime_owner());
        Ok(client)
    }
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn jsonc_has_a_separate_file_and_preserves_unknown_fields() {
        let temporary = tempfile::tempdir().unwrap();
        let services = crate::test_support::TestServices::persistent_with_notes_base(
            temporary.path(),
            temporary.path().join("notes"),
        )
        .unwrap();
        let manager = &services.node;
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
        let settings = services.settings.test_settings_path().unwrap();
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
