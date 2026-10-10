use crate::aow::AowManager;
use anyhow::Result;
use aow_zed::{AcpHost, AcpService};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Host {
    manager: AowManager,
}

#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        let manager = self.manager.clone();
        Ok(tokio::task::spawn_blocking(move || manager.zed_settings())
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

pub(crate) fn service(manager: AowManager, directory: Option<PathBuf>) -> Result<AcpService> {
    AcpService::new(
        Arc::new(Host { manager }),
        directory.map(|directory| directory.join("zed")),
    )
}
