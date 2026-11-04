use super::*;
use std::{path::Path, sync::atomic::AtomicBool, time::Duration};
use tokio::sync::Notify;

struct Host {
    first: AtomicBool,
    entered: Notify,
    release: Notify,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        if self.first.swap(false, Ordering::AcqRel) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Ok(r#"{"agent_servers":{"fixture":{"type":"custom","command":"/bin/false"}}}"#.into())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &Path, _: &Path) -> Result<String> {
        anyhow::bail!("unexpected file read")
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> Result<()> {
        anyhow::bail!("unexpected file write")
    }
}

#[tokio::test]
async fn shutdown_does_not_deadlock_a_connection_waiting_to_prepare_its_adapter() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let host = Arc::new(Host {
        first: AtomicBool::new(true),
        entered: Notify::new(),
        release: Notify::new(),
    });
    let service = AcpService::new(host.clone(), None)?;
    let connection = {
        let service = service.clone();
        let cwd = directory.path().to_owned();
        tokio::spawn(async move { service.connect("fixture", cwd).await })
    };
    host.entered.notified().await;
    let shutdown = {
        let service = service.clone();
        tokio::spawn(async move { service.shutdown().await })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while !service.adapters.inner.shutting_down.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    host.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), shutdown).await???;
    let error = connection.await?.unwrap_err();
    assert!(error.to_string().contains("shutting down"), "{error:#}");
    let error = service.install_agent("fixture").await.unwrap_err();
    assert!(error.to_string().contains("shutting down"), "{error:#}");
    Ok(())
}
