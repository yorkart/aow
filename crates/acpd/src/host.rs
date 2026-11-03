use anyhow::Result;
use aow_acp::{AcpHost, TurnCompletion};
use aow_acpd_client::Event;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::broadcast;

pub(super) struct Host {
    pub root: PathBuf,
    pub probe: bool,
    pub completed: broadcast::Sender<Event>,
    pub filesystem: Arc<tokio::sync::RwLock<HashSet<PathBuf>>>,
    pub stopping: AtomicBool,
}
impl Host {
    pub async fn set_workspace_removing(&self, cwd: &str, removing: bool) {
        // The write guard drains existing file callbacks before cleanup is acknowledged.
        let mut blocked = self.filesystem.write().await;
        if removing {
            blocked.insert(cwd.into());
        } else {
            blocked.remove(Path::new(cwd));
        }
    }
    pub fn outbox(&self) -> PathBuf {
        self.root.join("data/acp/completions")
    }
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
        let configured = match tokio::fs::read(self.root.join("aow-settings.json")).await {
            Ok(bytes) => serde_json::from_slice::<Settings>(&bytes)?.execution_path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let path = match configured {
            Some(path) => path,
            None => aow_agents::environment::discover_path().await,
        };
        Ok(Some(std::env::join_paths(path)?))
    }
    fn execution_environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(
            "AOW_STATE_DIR".into(),
            self.root.to_string_lossy().into_owned(),
        )])
    }
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String> {
        anyhow::ensure!(!self.probe, "Connection tests do not read workspace files");
        let blocked = self.filesystem.read().await;
        anyhow::ensure!(
            !blocked.contains(cwd) && !self.stopping.load(Ordering::Acquire),
            "ACP workspace is unavailable for file access"
        );
        Ok(
            aow_filesystem::read_text_file(path, aow_filesystem::DEFAULT_MAX_TEXT_BYTES)
                .await?
                .content,
        )
    }
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()> {
        anyhow::ensure!(!self.probe, "Connection tests do not write workspace files");
        let blocked = self.filesystem.read().await;
        anyhow::ensure!(
            !blocked.contains(cwd) && !self.stopping.load(Ordering::Acquire),
            "ACP workspace is unavailable for file access"
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
    async fn turn_completed(&self, completion: TurnCompletion) {
        if self.probe {
            return;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let event = Event::Completed {
            id: id.clone(),
            completion,
        };
        let result: Result<()> = async {
            let directory = self.outbox();
            tokio::fs::create_dir_all(&directory).await?;
            let bytes = serde_json::to_vec(&event)?;
            aow_filesystem::write_atomic_stream(
                directory.join(format!("{id}.json")),
                None,
                futures_util::stream::iter([Ok::<_, String>(bytes::Bytes::from(bytes))]),
            )
            .await?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            tracing::error!(%error, "failed to persist ACP completion notification");
        }
        let _ = self.completed.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn removal_and_shutdown_fence_agent_file_callbacks() {
        let root = tempfile::tempdir().unwrap();
        let host = Host {
            root: root.path().into(),
            probe: false,
            completed: broadcast::channel(1).0,
            filesystem: Default::default(),
            stopping: AtomicBool::new(false),
        };
        let path = root.path().join("source.txt");
        host.write_text_file(root.path(), &path, "before")
            .await
            .unwrap();
        host.set_workspace_removing(root.path().to_str().unwrap(), true)
            .await;
        assert!(
            host.write_text_file(root.path(), &path, "late callback")
                .await
                .is_err()
        );
        assert!(host.read_text_file(root.path(), &path).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "before");
        host.set_workspace_removing(root.path().to_str().unwrap(), false)
            .await;
        assert_eq!(
            host.read_text_file(root.path(), &path).await.unwrap(),
            "before"
        );
        host.stopping.store(true, Ordering::Release);
        assert!(
            host.write_text_file(root.path(), &path, "shutdown callback")
                .await
                .is_err()
        );
    }
}
