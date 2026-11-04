use anyhow::Result;
use aow_acp_client::{AcpHost, AcpService};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Host {
    marker: PathBuf,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(serde_json::json!({"agent_servers":{"fixture":{
            "type":"custom", "command":"/bin/sh",
            "args":["-c", "echo unexpected > \"$1\"", "fixture", self.marker]
        }}})
        .to_string())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &Path, _: &Path) -> Result<String> {
        anyhow::bail!("unexpected file callback")
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> Result<()> {
        anyhow::bail!("unexpected file callback")
    }
}

#[tokio::test]
async fn a_configured_command_cannot_fall_back_to_local_execution_without_a_connector() -> Result<()>
{
    let directory = tempfile::tempdir()?;
    let marker = directory.path().join("unexpected-execution");
    let service = AcpService::new(
        Arc::new(Host {
            marker: marker.clone(),
        }),
        None,
    )?;
    let error = service
        .connect("fixture", directory.path().into())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must provide an execution transport"),
        "{error:#}"
    );
    assert!(!marker.exists());
    service.shutdown().await?;
    Ok(())
}
