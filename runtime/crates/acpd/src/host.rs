use anyhow::Result;
use aow_acp::AcpHost;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) struct Host {
    pub root: PathBuf,
    pub stopping: AtomicBool,
    pub filesystem: tokio::sync::RwLock<std::collections::HashSet<PathBuf>>,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        match tokio::fs::read_to_string(self.root.join("aow-acp.json")).await {
            Ok(content) => Ok(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok("{\"agent_servers\":{}}".into())
            }
            Err(error) => Err(error.into()),
        }
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        #[derive(serde::Deserialize)]
        struct Settings {
            execution_path: Option<Vec<PathBuf>>,
        }
        let paths = match tokio::fs::read(self.root.join("aow-settings.json")).await {
            Ok(bytes) => serde_json::from_slice::<Settings>(&bytes)?.execution_path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Some(std::env::join_paths(match paths {
            Some(paths) => paths,
            None => aow_agents::environment::discover_path().await,
        })?))
    }
    fn execution_environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(
            "AOW_STATE_DIR".into(),
            self.root.to_string_lossy().into_owned(),
        )])
    }
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String> {
        let blocked = self.filesystem.read().await;
        anyhow::ensure!(!blocked.contains(cwd), "Workspace is being removed");
        anyhow::ensure!(
            !self.stopping.load(Ordering::Acquire),
            "Runtime is stopping"
        );
        Ok(
            aow_filesystem::read_text_file(path, aow_filesystem::DEFAULT_MAX_TEXT_BYTES)
                .await?
                .content,
        )
    }
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()> {
        let blocked = self.filesystem.read().await;
        anyhow::ensure!(!blocked.contains(cwd), "Workspace is being removed");
        anyhow::ensure!(
            !self.stopping.load(Ordering::Acquire),
            "Runtime is stopping"
        );
        aow_filesystem::write_atomic_stream(
            path,
            None,
            futures_util::stream::iter([Ok::<_, String>(bytes::Bytes::copy_from_slice(
                content.as_bytes(),
            ))]),
        )
        .await?;
        Ok(())
    }
}
