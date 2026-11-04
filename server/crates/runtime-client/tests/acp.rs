use anyhow::Result;
use aow_runtime_client::{
    RuntimeClient,
    acp::{AcpClient, AcpHost, ExecutionEnvironment, SessionCreation, TurnCompletion},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

struct Host {
    runtime: RuntimeClient,
    completed: mpsc::UnboundedSender<TurnCompletion>,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        anyhow::bail!("Configuration must be read from the node")
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        anyhow::bail!("PATH must be read from the node")
    }
    async fn turn_completed(&self, completion: TurnCompletion) {
        let _ = self.completed.send(completion);
    }
    async fn read_text_file(&self, cwd: &Path, path: &Path) -> Result<String> {
        let response: serde_json::Value = self
            .runtime
            .post_json("/v1/files/read", &json!({"cwd":cwd,"path":path}))
            .await?;
        Ok(response["content"].as_str().unwrap().into())
    }
    async fn write_text_file(&self, cwd: &Path, path: &Path, content: &str) -> Result<()> {
        let _: serde_json::Value = self
            .runtime
            .post_json(
                "/v1/files/write",
                &json!({"cwd":cwd,"path":path,"content":content}),
            )
            .await?;
        Ok(())
    }
}
struct Node {
    runtime: RuntimeClient,
    fixture: Option<aow_runtime_client::testing::TestRuntime>,
    root: tempfile::TempDir,
}
impl Node {
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent.mjs");
        std::fs::write(root.path().join("aow-acp.json"),json!({"default_agent_id":"fixture","agent_servers":{"fixture":{"type":"custom","command":"node","args":[script]}}}).to_string()).unwrap();
        std::fs::write(root.path().join("aow-settings.json"),json!({"execution_path":std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect::<Vec<_>>()} ).to_string()).unwrap();
        let fixture = aow_runtime_client::testing::TestRuntime::with_state_dir(
            &root.path().join("connection.json"),
            root.path().into(),
        )
        .await;
        let runtime = RuntimeClient::new(fixture.endpoint.clone());
        Self {
            runtime,
            fixture: Some(fixture),
            root,
        }
    }
    async fn stop(mut self) {
        self.fixture.take().unwrap().shutdown().await.unwrap();
    }
}

async fn snapshot_until(
    client: &AcpClient,
    id: &str,
    predicate: impl Fn(&aow_runtime_client::acp::SessionSnapshot) -> bool,
) -> aow_runtime_client::acp::SessionSnapshot {
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
async fn native_acp_sdk_uses_node_configuration_permissions_and_client_history() {
    let node = Node::start().await;
    let history = tempfile::tempdir().unwrap();
    let home = node.root.path().join("role-home");
    std::fs::create_dir(&home).unwrap();
    let (completed, mut completions) = mpsc::unbounded_channel();
    let host = Arc::new(Host {
        runtime: node.runtime.clone(),
        completed,
    });
    let client = AcpClient::with_runtime(
        node.runtime.clone(),
        host.clone(),
        Some(history.path().join("history")),
    )
    .unwrap();
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
    let reopened = AcpClient::with_runtime(
        node.runtime.clone(),
        host,
        Some(history.path().join("history")),
    )
    .unwrap();
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
    let client = AcpClient::with_runtime(
        node.runtime.clone(),
        Arc::new(Host {
            runtime: node.runtime.clone(),
            completed,
        }),
        None,
    )
    .unwrap();
    let session = client
        .prepare_session("fixture", node.root.path().into())
        .await
        .unwrap();
    client.resume(&session.thread_id).await.unwrap();
    let pid = node.runtime.acp_instances().await.unwrap()[0].pid;
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
