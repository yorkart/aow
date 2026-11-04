use crate::aow::AowManager;
use anyhow::Result;
use aow_runtime_client::acp::AcpClient;
use aow_runtime_client::acp::AcpHost;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Host {
    runtime: Option<aow_runtime_client::RuntimeClient>,
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
    async fn turn_completed(&self, completion: aow_runtime_client::acp::TurnCompletion) {
        super::notifications::dispatch(&self.manager, &self.page, completion).await;
    }
    fn execution_environment(&self) -> Result<std::collections::BTreeMap<String, String>> {
        if self.runtime.is_some() {
            return self.manager.cli_access().cached_environment();
        }
        Ok(self
            .state_directory
            .as_ref()
            .map(|path| ("AOW_STATE_DIR".into(), path.to_string_lossy().into_owned()))
            .into_iter()
            .collect())
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
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String> {
        let _access = self.manager.filesystem_access().await;
        if let Some(runtime) = &self.runtime {
            let response: serde_json::Value = runtime
                .post_json(
                    "/v1/files/read",
                    &serde_json::json!({"cwd":cwd,"path":path}),
                )
                .await?;
            return Ok(response["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid node file response"))?
                .into());
        }
        Ok(aow_filesystem::read_text_file(
            path.to_path_buf(),
            aow_filesystem::DEFAULT_MAX_TEXT_BYTES,
        )
        .await?
        .content)
    }
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()> {
        let _access = self.manager.filesystem_access().await;
        if let Some(runtime) = &self.runtime {
            let _: serde_json::Value = runtime
                .post_json(
                    "/v1/files/write",
                    &serde_json::json!({"cwd":cwd,"path":path,"content":content}),
                )
                .await?;
            return Ok(());
        }
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
) -> Result<AcpClient> {
    service_with_runtime(manager, directory, page, workspace_events, None)
}

pub(crate) fn service_with_runtime(
    manager: AowManager,
    directory: Option<PathBuf>,
    page: tokio::sync::broadcast::Sender<crate::terminal::notifications::TaskStopNotification>,
    workspace_events: crate::workspace_events::WorkspaceEvents,
    runtime: Option<aow_runtime_client::RuntimeClient>,
) -> Result<AcpClient> {
    let host = Arc::new(Host {
        manager,
        page,
        workspace_events,
        runtime: runtime.clone(),
        state_directory: directory.as_ref().map(std::fs::canonicalize).transpose()?,
    });
    let state = directory
        .as_ref()
        .map(|directory| directory.join("data/acp"));
    match runtime {
        Some(runtime) => AcpClient::with_runtime(runtime, host, state),
        None => AcpClient::for_test_host(host, state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aow_runtime_client::{RuntimeClient, testing::TestRuntime};
    use serde_json::json;
    use std::time::Duration;

    #[tokio::test]
    async fn server_uses_node_configuration_and_saves_native_acp_history_locally() {
        let caller = tempfile::tempdir().unwrap();
        let node = TestRuntime::start(&caller.path().join("node.json")).await;
        let runtime = RuntimeClient::new(node.endpoint.clone());
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../runtime-client/tests/fixtures/agent.mjs");
        let settings: serde_json::Value = runtime.get_json("/v1/acp/settings").await.unwrap();
        let _: serde_json::Value = runtime
            .put_json(
                "/v1/acp/settings",
                &json!({
                    "revision":settings["revision"],
                    "content":json!({"default_agent_id":"node-agent","agent_servers":{"node-agent":{
                        "type":"custom","command":"node","args":[script],"env":{"AOW_TEST_CALLBACK_REPORT":caller.path().join("callback-env.json")}}}}).to_string()
                }),
            )
            .await
            .unwrap();
        // The caller has no adapter configuration. Production AppState must use
        // the node catalog and transport rather than resolving a local command.
        let state = crate::AppState::with_runtime(
            PathBuf::new(),
            caller.path().into(),
            node.endpoint.clone(),
        )
        .unwrap()
        .with_cli_callback("http://callback-server:9800", None)
        .unwrap();
        assert_eq!(state.acp.agents().await.unwrap()[0].id, "node-agent");
        let session = state
            .acp
            .prepare_session("node-agent", caller.path().into())
            .await
            .unwrap();
        let session = state.acp.resume(&session.thread_id).await.unwrap();
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(caller.path().join("callback-env.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(report["url"], "http://callback-server:9800");
        assert_eq!(report["has_token"], true);
        state.acp.resume(&session.thread_id).await.unwrap();
        let reused: serde_json::Value = serde_json::from_slice(
            &std::fs::read(caller.path().join("callback-env.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            report["pid"], reused["pid"],
            "A valid cached callback grant must not replace an ACP process"
        );

        state
            .acp
            .start_prompt_once(
                &session.thread_id,
                "server-turn",
                serde_json::from_value(json!([{"type":"text","text":"hello"}])).unwrap(),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = state.acp.snapshot(&session.thread_id).await.unwrap();
                if let Some(permission) = snapshot.permissions.first() {
                    state
                        .acp
                        .answer(
                            &session.thread_id,
                            &permission.id,
                            json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
                        )
                        .await
                        .unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            loop {
                let snapshot = state.acp.snapshot(&session.thread_id).await.unwrap();
                if snapshot.status == "idle" {
                    assert!(
                        snapshot
                            .entries
                            .iter()
                            .any(|entry| entry.kind == "assistant"
                                && entry.content["text"] == "permission received")
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        state.flush_acp_history().await.unwrap();
        drop(state);
        let reopened = crate::AppState::with_runtime(
            PathBuf::new(),
            caller.path().into(),
            node.endpoint.clone(),
        )
        .unwrap();
        let snapshot = reopened.acp.snapshot(&session.thread_id).await.unwrap();
        assert!(snapshot.entries.iter().any(
            |entry| entry.kind == "assistant" && entry.content["text"] == "permission received"
        ));
        reopened.flush_acp_history().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
