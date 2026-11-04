use crate::aow::AowManager;
use anyhow::Result;
use aow_runtime_client::acp::{AcpClient, AcpObserver};
use std::{path::PathBuf, sync::Arc};
struct Observer {
    manager: AowManager,
    cursor_path: Option<PathBuf>,
    page: tokio::sync::broadcast::Sender<crate::terminal::notifications::TaskStopNotification>,
    workspace_events: crate::workspace_events::WorkspaceEvents,
}
#[async_trait::async_trait]
impl AcpObserver for Observer {
    fn session_metadata_changed(&self) {
        self.workspace_events.instances_changed();
    }
    async fn turn_completed(&self, completion: aow_runtime_client::acp::TurnCompletion) {
        super::notifications::dispatch(&self.manager, &self.page, completion).await;
    }
    fn completion_cursor(&self) -> Result<Option<aow_runtime_client::acp::ConversationCursor>> {
        let Some(path) = &self.cursor_path else {
            return Ok(None);
        };
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn save_completion_cursor(
        &self,
        cursor: &aow_runtime_client::acp::ConversationCursor,
    ) -> Result<()> {
        let Some(path) = &self.cursor_path else {
            return Ok(());
        };
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec(cursor)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        std::fs::File::open(path.parent().unwrap())?.sync_all()?;
        Ok(())
    }
    fn callback_environment(&self) -> Result<std::collections::BTreeMap<String, String>> {
        self.manager.cli_access().cached_environment()
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
    let observer = Arc::new(Observer {
        manager: manager.clone(),
        cursor_path: directory.map(|root| root.join("state/acp-completion-cursor.json")),
        page,
        workspace_events,
    });
    match runtime {
        Some(runtime) => Ok(AcpClient::with_observer(runtime, Some(observer))),
        None => {
            #[cfg(test)]
            {
                let client = manager.test_acp_client()?;
                Ok(client.with_test_observer(observer))
            }
            #[cfg(not(test))]
            {
                Ok(AcpClient::with_observer(
                    aow_runtime_client::RuntimeClient::new(
                        aow_runtime_client::RuntimeEndpoint::unavailable(),
                    ),
                    Some(observer),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aow_runtime_client::{RuntimeClient, testing::TestRuntime};
    use serde_json::json;
    use std::time::Duration;

    #[tokio::test]
    async fn server_uses_node_configuration_and_remote_conversation_history() {
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
