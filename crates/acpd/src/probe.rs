use crate::host::Host;
use anyhow::{Result, bail, ensure};
use aow_acp::{AcpService, ExecutionEnvironment, SessionCreation};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Probe(AcpService);
impl Drop for Probe {
    fn drop(&mut self) {
        for session in self.0.sessions(None) {
            if let Ok(Some(connection)) = self.0.session_connection_info(&session.thread_id) {
                let _ = self.0.disconnect(&connection.id);
            }
        }
    }
}

pub(super) async fn run(
    host: Arc<Host>,
    agent: String,
    cwd: PathBuf,
    execution: ExecutionEnvironment,
) -> Result<()> {
    let host = Arc::new(Host {
        root: host.root.clone(),
        probe: true,
        filesystem: host.filesystem.clone(),
        stopping: std::sync::atomic::AtomicBool::new(false),
        completed: host.completed.clone(),
    });
    let service = Probe(AcpService::with_storage(
        host.clone(),
        None,
        Some(host.root.join("cache/acp")),
    )?);
    tokio::time::timeout(Duration::from_secs(60), async {
        let session = service.0.prepare_session_with_options(&agent, cwd, SessionCreation { execution_environment: Some(execution), ..Default::default() }).await?;
        service.0.resume(&session.thread_id).await?;
        service.0.start_prompt(&session.thread_id, serde_json::from_value(serde_json::json!([{"type":"text", "text":"ping"}]))?)?;
        loop {
            let snapshot = service.0.snapshot(&session.thread_id)?;
            ensure!(snapshot.permissions.is_empty(), "The runtime requested permission during the connection test. Complete local setup first.");
            if let Some(error) = snapshot.error { bail!("{error}"); }
            match snapshot.status.as_str() {
                "working" => tokio::time::sleep(Duration::from_millis(40)).await,
                "idle" => {
                    ensure!(snapshot.entries.iter().any(|entry| entry.kind == "assistant" && entry.content["text"].as_str().is_some_and(|text| !text.trim().is_empty())), "The runtime did not return a connection-test response");
                    return Ok(());
                },
                status => bail!("Runtime disconnected during connection test ({status})"),
            }
        }
    }).await?
}
