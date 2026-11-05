use anyhow::Result;
use aow_acp_runtime_core::AcpHost;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) struct Host {
    pub _ownership: std::sync::Arc<crate::runtime::NodeOwnership>,
    pub root: PathBuf,
    pub stopping: AtomicBool,
    pub completions: crate::conversations::journal::CompletionLog,
    pub callback_environment: std::sync::RwLock<BTreeMap<String, String>>,
    pub filesystem: tokio::sync::RwLock<std::collections::HashSet<PathBuf>>,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn turn_completed(&self, completion: aow_acp_runtime_core::TurnCompletion) {
        if let Err(error) = self.completions.append(&completion) {
            tracing::error!(%error, "Failed to persist conversation completion");
        }
    }

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
            None => aow_agent_runtime_core::environment::discover_path().await,
        })?))
    }
    fn execution_environment(&self) -> BTreeMap<String, String> {
        let mut environment = self.callback_environment.read().unwrap().clone();
        environment.extend(BTreeMap::from([(
            "AOW_STATE_DIR".into(),
            self.root.to_string_lossy().into_owned(),
        )]));
        environment
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

impl Host {
    pub(crate) fn set_callback_environment(
        &self,
        environment: BTreeMap<String, String>,
    ) -> Result<()> {
        anyhow::ensure!(
            environment.len() <= 8
                && environment.iter().all(|(key, value)| matches!(
                    key.as_str(),
                    "AOW_SERVER_URL" | "AOW_SERVER_TOKEN" | "AOW_SERVER_TOKEN_FILE"
                ) && !value.contains('\0')
                    && value.len() < 16384),
            "Invalid business callback environment"
        );
        *self.callback_environment.write().unwrap() = environment;
        Ok(())
    }
}
