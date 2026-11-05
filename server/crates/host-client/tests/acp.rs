use anyhow::Result;
use aow_host_client::testing::HostTransport;
use aow_host_client::{
    HostClient,
    acp::{AcpClient, AcpObserver, ExecutionEnvironment, SessionCreation, TurnCompletion},
};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;

#[tokio::test]
async fn shared_fixture_does_not_restore_stale_host_settings_during_publication() {
    struct Settings(std::sync::RwLock<String>);
    #[async_trait::async_trait]
    impl aow_host_client::testing::AcpFixture for Settings {
        async fn settings(&self) -> Result<String> {
            Ok(self.0.read().unwrap().clone())
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let old = json!({"agent_servers":{"test":{"type":"custom","command":"node"}}}).to_string();
    let host = Arc::new(Settings(std::sync::RwLock::new(old)));
    let first = AcpClient::for_test_host(host.clone(), Some(directory.path().into())).unwrap();
    let second = AcpClient::for_test_host(host.clone(), Some(directory.path().into())).unwrap();
    let before = first.settings().await.unwrap();
    let published = json!({"agent_servers":{"test":{"type":"registry"}}}).to_string();
    first
        .save_settings(published.clone(), before.revision)
        .await
        .unwrap();

    // Server's test helper validates/publishes remotely before committing its
    // local source. Another client may issue an event refresh in that window.
    assert_eq!(second.settings().await.unwrap().content, published);
    *host.0.write().unwrap() = published.clone();
    assert_eq!(first.settings().await.unwrap().content, published);

    let removed = json!({"agent_servers":{}}).to_string();
    *host.0.write().unwrap() = removed.clone();
    assert_eq!(second.settings().await.unwrap().content, removed);
    assert_eq!(first.settings().await.unwrap().content, removed);
}

struct Host {
    completed: mpsc::UnboundedSender<TurnCompletion>,
}
#[async_trait::async_trait]
impl AcpObserver for Host {
    async fn turn_completed(&self, completion: TurnCompletion) {
        let _ = self.completed.send(completion);
    }
}

struct Node {
    runtime: HostClient,
    fixture: Option<aow_host_client::testing::TestHost>,
    root: tempfile::TempDir,
}
impl Node {
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent.mjs");
        std::fs::write(root.path().join("aow-acp.json"),json!({"default_agent_id":"fixture","agent_servers":{"fixture":{"type":"custom","command":"node","args":[script],"env":{"AOW_TEST_CALLBACK_REPORT":root.path().join("adapter.json")}}}}).to_string()).unwrap();
        std::fs::write(root.path().join("aow-settings.json"),json!({"execution_path":std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect::<Vec<_>>()} ).to_string()).unwrap();
        let fixture = aow_host_client::testing::TestHost::with_state_dir(
            &root.path().join("connection.json"),
            root.path().into(),
        )
        .await;
        let runtime = HostClient::new(fixture.endpoint.clone());
        Self {
            runtime,
            fixture: Some(fixture),
            root,
        }
    }
    async fn restart(&mut self) {
        self.fixture.take().unwrap().shutdown().await.unwrap();
        let fixture = aow_host_client::testing::TestHost::with_state_dir(
            &self.root.path().join("connection.json"),
            self.root.path().into(),
        )
        .await;
        self.runtime = HostClient::new(fixture.endpoint.clone());
        self.fixture = Some(fixture);
    }
    async fn stop(mut self) {
        self.fixture.take().unwrap().shutdown().await.unwrap();
    }
}

async fn snapshot_until(
    client: &AcpClient,
    id: &str,
    predicate: impl Fn(&aow_host_client::acp::SessionSnapshot) -> bool,
) -> aow_host_client::acp::SessionSnapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = client.snapshot(id).await.unwrap();
            if predicate(&snapshot) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn native_acp_sdk_uses_node_configuration_permissions_and_node_history() {
    let node = Node::start().await;
    let history = tempfile::tempdir().unwrap();
    let home = node.root.path().join("role-home");
    std::fs::create_dir(&home).unwrap();
    let (completed, mut completions) = mpsc::unbounded_channel();
    let host = Arc::new(Host { completed });
    let client = AcpClient::with_observer(node.runtime.clone(), Some(host.clone()));
    assert_eq!(client.agents().await.unwrap()[0].id, "fixture");
    let session = client
        .prepare_session_with_options(
            "fixture",
            node.root.path().into(),
            SessionCreation {
                execution_environment: Some(ExecutionEnvironment {
                    home,
                    env: BTreeMap::new(),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let session = client.resume(&session.thread_id).await.unwrap();
    assert_eq!(session.session_id.as_deref(), Some("native-session"));
    client
        .start_prompt_once(
            &session.thread_id,
            "first",
            serde_json::from_value(json!([{"type":"text","text":"hello"}])).unwrap(),
        )
        .await
        .unwrap();
    let pending = snapshot_until(&client, &session.thread_id, |snapshot| {
        !snapshot.permissions.is_empty()
    })
    .await;
    client
        .answer(
            &session.thread_id,
            &pending.permissions[0].id,
            json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
        )
        .await
        .unwrap();
    let done = snapshot_until(&client, &session.thread_id, |snapshot| {
        snapshot.status == "idle"
    })
    .await;
    assert!(
        done.entries.iter().any(
            |entry| entry.kind == "assistant" && entry.content["text"] == "permission received"
        )
    );
    let completion = tokio::time::timeout(Duration::from_secs(5), completions.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completion.session.thread_id, session.thread_id);
    assert!(
        !client
            .logs(
                &client
                    .session_connection_info(&session.thread_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .id
            )
            .await
            .unwrap()
            .is_empty()
    );
    client.flush_threads_on_quit().await.unwrap();
    assert!(!history.path().join("history").exists());
    assert!(node.root.path().join("data/acp/threads.sqlite").exists());
    let reopened = AcpClient::with_observer(node.runtime.clone(), Some(host));
    assert_eq!(
        reopened
            .snapshot(&session.thread_id)
            .await
            .unwrap()
            .thread_id,
        session.thread_id
    );
    reopened.flush_threads_on_quit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !node.runtime.acp_instances().await.unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    node.stop().await;
}
#[tokio::test]
async fn workspace_fence_reaps_adapter_and_blocks_node_file_callbacks() {
    let node = Node::start().await;
    let (completed, _) = mpsc::unbounded_channel();
    let client = AcpClient::with_observer(node.runtime.clone(), Some(Arc::new(Host { completed })));
    let session = client
        .prepare_session("fixture", node.root.path().into())
        .await
        .unwrap();
    client.resume(&session.thread_id).await.unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(node.root.path().join("adapter.json")).unwrap())
            .unwrap();
    let pid = report["pid"].as_u64().unwrap();
    let cwd = std::fs::canonicalize(node.root.path()).unwrap();
    client
        .set_workspace_removing(cwd.to_str().unwrap(), true)
        .await
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    let response: Result<serde_json::Value, _> = node
        .runtime
        .post_json(
            "/v1/files/write",
            &json!({"cwd":cwd,"path":cwd.join("late.txt"),"content":"late"}),
        )
        .await;
    assert!(response.is_err());
    assert!(!cwd.join("late.txt").exists());
    assert!(
        client
            .prepare_session("fixture", cwd.clone())
            .await
            .is_err()
    );
    client
        .delete_workspace(cwd.to_str().unwrap())
        .await
        .unwrap();
    client
        .set_workspace_removing(cwd.to_str().unwrap(), false)
        .await
        .unwrap();
    assert!(client.prepare_session("fixture", cwd).await.is_ok());
    client.flush_threads_on_quit().await.unwrap();
    node.stop().await;
}

#[tokio::test]
async fn reconnect_and_node_restart_preserve_permissions_history_and_admission() {
    let mut node = Node::start().await;
    let client = AcpClient::new(node.runtime.clone());
    let session = client
        .prepare_session("fixture", node.root.path().into())
        .await
        .unwrap();
    client.resume(&session.thread_id).await.unwrap();
    let content = serde_json::from_value(json!([{"type":"text","text":"durable turn"}])).unwrap();
    client
        .start_prompt_once(&session.thread_id, "durable-request", content)
        .await
        .unwrap();
    let pending = snapshot_until(&client, &session.thread_id, |snapshot| {
        !snapshot.permissions.is_empty()
    })
    .await;
    drop(client);
    let replacement = AcpClient::new(node.runtime.clone());
    replacement
        .answer(
            &session.thread_id,
            &pending.permissions[0].id,
            json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
        )
        .await
        .unwrap();
    let finished = snapshot_until(&replacement, &session.thread_id, |snapshot| {
        snapshot.status == "idle"
    })
    .await;
    let count = finished.entries.len();
    assert_eq!(
        finished.prompt_receipts["durable-request"].status,
        "completed"
    );
    drop(replacement);
    node.restart().await;
    let restored = AcpClient::new(node.runtime.clone());
    let snapshot = restored.snapshot(&session.thread_id).await.unwrap();
    assert_eq!(snapshot.entries.len(), count);
    assert!(snapshot.permissions.is_empty());
    assert_eq!(restored.sessions(None).await.unwrap().len(), 1);
    restored
        .start_prompt_once(
            &session.thread_id,
            "durable-request",
            serde_json::from_value(json!([{"type":"text","text":"must not execute twice"}]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        restored
            .snapshot(&session.thread_id)
            .await
            .unwrap()
            .entries
            .len(),
        count
    );
    assert!(
        restored
            .session_connection_info(&session.thread_id)
            .await
            .unwrap()
            .is_none()
    );
    drop(restored);
    node.stop().await;
}

#[tokio::test]
async fn managed_socket_uses_native_acp_and_drains_updates_before_prompt_response() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let node = Node::start().await;
    let mut socket = node
        .runtime
        .websocket("/v1/acp/conversations/acp")
        .await
        .unwrap();
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}).to_string().into())).await.unwrap();
    let Message::Text(ready) = socket.next().await.unwrap().unwrap() else {
        panic!("initialize response");
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&ready).unwrap()["result"]["protocolVersion"],
        1
    );
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":"new","method":"session/new","params":{"cwd":node.root.path(),"mcpServers":[]}}).to_string().into())).await.unwrap();
    let thread = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            if value["id"] == "new" {
                assert!(value.get("error").is_none(), "{value}");
                break value["result"]["sessionId"].as_str().unwrap().to_owned();
            }
        }
    })
    .await
    .unwrap();
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":"prompt","method":"session/prompt","params":{"sessionId":thread,"prompt":[{"type":"text","text":"hello"}]}}).to_string().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut got_final_text = false;
        loop {
            let Message::Text(text) = socket.next().await.unwrap().unwrap() else { continue; };
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            match value["method"].as_str() {
                Some("session/request_permission") => {
                    assert_eq!(value["params"]["sessionId"], thread);
                    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":value["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow"}}}).to_string().into())).await.unwrap();
                }
                Some("session/update") if value["params"]["update"]["content"]["text"] == "permission received" => { got_final_text = true; }
                _ => {}
            }
            if value["id"] == "prompt" && value.get("method").is_none() {
                assert!(got_final_text, "Final response overtook its streamed notification: {value}");
                assert_eq!(value["result"]["stopReason"], "end_turn");
                break;
            }
        }
    }).await.unwrap();
    socket.close(None).await.unwrap();
    let client = AcpClient::new(node.runtime.clone());
    assert!(
        client
            .snapshot(&thread)
            .await
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.content["text"] == "permission received")
    );
    drop(client);
    node.stop().await;
}

#[tokio::test]
async fn completion_outbox_replays_after_disconnect_and_node_restart() {
    use aow_host_client::acp::ConversationEvent;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let mut node = Node::start().await;
    let mut events = node
        .runtime
        .websocket("/v1/acp/conversations/events")
        .await
        .unwrap();
    let Message::Text(text) = events.next().await.unwrap().unwrap() else {
        panic!("ready");
    };
    let ConversationEvent::Ready {
        journal_id, after, ..
    } = serde_json::from_str(&text).unwrap()
    else {
        panic!("ready frame");
    };
    events.close(None).await.unwrap();
    let client = AcpClient::new(node.runtime.clone());
    let session = client
        .prepare_session("fixture", node.root.path().into())
        .await
        .unwrap();
    client.resume(&session.thread_id).await.unwrap();
    client
        .start_prompt(
            &session.thread_id,
            serde_json::from_value(json!([{"type":"text","text":"outbox"}])).unwrap(),
        )
        .await
        .unwrap();
    let pending = snapshot_until(&client, &session.thread_id, |snapshot| {
        !snapshot.permissions.is_empty()
    })
    .await;
    client
        .answer(
            &session.thread_id,
            &pending.permissions[0].id,
            json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
        )
        .await
        .unwrap();
    snapshot_until(&client, &session.thread_id, |snapshot| {
        snapshot.status == "idle"
    })
    .await;
    drop(client);
    node.restart().await;
    let mut events = node
        .runtime
        .websocket(&format!(
            "/v1/acp/conversations/events?journal_id={journal_id}&after={after}"
        ))
        .await
        .unwrap();
    let completion = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.next().await.unwrap().unwrap() {
                Message::Text(text) => {
                    if let ConversationEvent::Completion {
                        journal_id: actual,
                        sequence,
                        completion,
                    } = serde_json::from_str(&text).unwrap()
                    {
                        assert_eq!(actual, journal_id);
                        assert_eq!(sequence, after + 1);
                        break completion;
                    }
                }
                Message::Ping(data) => events.send(Message::Pong(data)).await.unwrap(),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(completion.session.thread_id, session.thread_id);
    assert_eq!(
        completion.conclusion.as_deref(),
        Some("permission received")
    );
    events.close(None).await.unwrap();
    assert!(
        node.runtime
            .websocket("/v1/acp/conversations/events?journal_id=another-node&after=0")
            .await
            .is_err()
    );
    node.stop().await;
}

#[tokio::test]
async fn management_errors_preserve_the_callers_language_across_the_node_boundary() {
    let node = Node::start().await;
    let client = AcpClient::new(node.runtime.clone());
    aow_i18n::scope(aow_i18n::Language::Chinese, async {
        let error = client.select_agent_id(Some("")).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            aow_i18n::text("acp.acp_agent_id_cannot_be_empty")
        );
    })
    .await;
    drop(client);
    node.stop().await;
}
