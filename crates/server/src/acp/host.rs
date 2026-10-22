use crate::aow::AowManager;
use anyhow::Result;
use aow_acp::{AcpHost, AcpService};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Host {
    manager: AowManager,
    state_directory: Option<PathBuf>,
    page: tokio::sync::broadcast::Sender<crate::terminal::notifications::TaskStopNotification>,
    workspace_events: crate::workspace_events::WorkspaceEvents,
}

#[async_trait::async_trait]
impl AcpHost for Host {
    fn session_metadata_changed(&self) {
        self.workspace_events.instances_changed();
    }
    async fn turn_completed(&self, completion: aow_acp::TurnCompletion) {
        super::notifications::dispatch(&self.manager, &self.page, completion).await;
    }
    fn execution_environment(&self) -> std::collections::BTreeMap<String, String> {
        self.state_directory
            .as_ref()
            .map(|path| ("AOW_STATE_DIR".into(), path.to_string_lossy().into_owned()))
            .into_iter()
            .collect()
    }
    async fn settings(&self) -> Result<String> {
        let manager = self.manager.clone();
        Ok(aow_i18n::spawn_blocking(move || manager.acp_settings())
            .await??
            .content)
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(Some(std::env::join_paths(
            self.manager.execution_path().await?,
        )?))
    }
    async fn read_text_file(&self, _cwd: &Path, path: &Path) -> Result<String> {
        let _access = self.manager.filesystem_access().await;
        Ok(aow_filesystem::read_text_file(
            path.to_path_buf(),
            aow_filesystem::DEFAULT_MAX_TEXT_BYTES,
        )
        .await?
        .content)
    }
    async fn write_text_file(&self, _cwd: &Path, path: &Path, content: &str) -> Result<()> {
        let _access = self.manager.filesystem_access().await;
        let data = bytes::Bytes::copy_from_slice(content.as_bytes());
        aow_filesystem::write_atomic_stream(
            path.to_path_buf(),
            None,
            futures_util::stream::iter([Ok::<_, String>(data)]),
        )
        .await?;
        Ok(())
    }
}

pub(crate) fn service(
    manager: AowManager,
    directory: Option<PathBuf>,
    page: tokio::sync::broadcast::Sender<crate::terminal::notifications::TaskStopNotification>,
    workspace_events: crate::workspace_events::WorkspaceEvents,
) -> Result<AcpService> {
    AcpService::new(
        Arc::new(Host {
            manager,
            page,
            workspace_events,
            state_directory: directory.as_ref().map(std::fs::canonicalize).transpose()?,
        }),
        directory.map(|directory| directory.join("acp")),
    )
}
