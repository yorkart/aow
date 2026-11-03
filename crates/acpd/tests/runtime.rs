use aow_acp::{AcpHost, AcpService, Content, ExecutionEnvironment, SessionCreation};
use aow_acpd_client::{AcpClient, Event};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    time::{sleep, timeout},
};

struct LegacyHost(String);
#[async_trait::async_trait]
impl AcpHost for LegacyHost {
    async fn settings(&self) -> anyhow::Result<String> {
        Ok(self.0.clone())
    }
    async fn execution_path(&self) -> anyhow::Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &Path, _: &Path) -> anyhow::Result<String> {
        unreachable!()
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> anyhow::Result<()> {
        unreachable!()
    }
}
fn content(text: &str) -> Vec<Content> {
    serde_json::from_value(json!([{"type":"text", "text":text}])).unwrap()
}
fn settings() -> String {
    let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("node"))
        .find(|path| path.is_file())
        .expect("Node.js test dependency");
    json!({"agent_servers":{"test":{"type":"custom", "command":executable, "args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../acp/tests/fixtures/lineage-agent.mjs")]}}}).to_string()
}
fn root() -> tempfile::TempDir {
    // Keep Unix socket paths within macOS's sockaddr_un limit.
    tempfile::Builder::new()
        .prefix("acpd-")
        .tempdir_in(if cfg!(target_os = "macos") {
            "/private/tmp"
        } else {
            "/tmp"
        })
        .unwrap()
}
fn spawn(root: &Path, socket: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_aow-acpd"))
        .arg("--state-dir")
        .arg(root)
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}
async fn ready(client: &AcpClient) {
    timeout(Duration::from_secs(15), async {
        loop {
            if client.health().await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
}
async fn idle(client: &AcpClient, id: &str) {
    timeout(Duration::from_secs(10), async {
        loop {
            if client.snapshot(id).await.unwrap().status == "idle" {
                break;
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
}
async fn stop(child: &mut Child) {
    assert_eq!(
        unsafe { libc::kill(child.id().unwrap() as i32, libc::SIGTERM) },
        0
    );
    assert!(
        timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn existing_history_and_running_prompt_survive_web_recreation_and_daemon_restart() {
    let directory = root();
    let root = directory.path();
    let workspace = root.join("workspace");
    let home = root.join("role-home");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let settings = settings();
    std::fs::write(root.join("aow-acp.json"), &settings).unwrap();
    // Seed the exact store layout used by the embedded implementation.
    let legacy = AcpService::with_storage(
        Arc::new(LegacyHost(settings)),
        Some(root.join("data/acp")),
        Some(root.join("cache/acp")),
    )
    .unwrap();
    let session = legacy
        .prepare_session_with_options(
            "test",
            workspace.clone(),
            SessionCreation {
                execution_environment: Some(ExecutionEnvironment {
                    home: home.clone(),
                    env: [("CUSTOM_AGENT_SETTING".into(), "role-value".into())].into(),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    legacy.flush_threads_on_quit().unwrap();
    drop(legacy);
    let socket = root.join("runtime/acpd/acpd.sock");
    let mut daemon = spawn(root, &socket);
    let client = AcpClient::remote(socket.clone());
    ready(&client).await;
    let first_instance = client.health().await.unwrap().instance_id;
    assert_eq!(client.sessions(None).await.unwrap().len(), 1);
    let id = &session.thread_id;
    assert_eq!(
        client.session_info(id).await.unwrap().status,
        "disconnected"
    );
    client.resume(id).await.unwrap();
    client
        .start_prompt_once(id, "request-1", content("wait"))
        .await
        .unwrap();
    let connection = client
        .session_connection_info(id)
        .await
        .unwrap()
        .unwrap()
        .id;
    // Exercise the production AppState path: it must not open or own the ACP store.
    let web = aow_server::AppState::with_daemon_options(
        Default::default(),
        root.into(),
        root.join("missing-terminald.sock"),
        socket.clone(),
    )
    .unwrap();
    web.flush_acp_history().await.unwrap();
    drop(web);
    let web = aow_server::AppState::with_daemon_options(
        Default::default(),
        root.into(),
        root.join("missing-terminald.sock"),
        socket.clone(),
    )
    .unwrap();
    let reconnected = AcpClient::remote(socket.clone());
    assert_eq!(reconnected.snapshot(id).await.unwrap().status, "working");
    assert_eq!(
        reconnected
            .session_connection_info(id)
            .await
            .unwrap()
            .unwrap()
            .id,
        connection
    );
    reconnected
        .start_prompt_once(id, "request-1", content("wait"))
        .await
        .unwrap();
    reconnected.cancel(id).await.unwrap();
    idle(&reconnected, id).await;
    let prompts = std::fs::read_to_string(workspace.join("lineage-prompts.jsonl")).unwrap();
    assert_eq!(prompts.lines().count(), 1);
    let prompt: serde_json::Value = serde_json::from_str(prompts.lines().next().unwrap()).unwrap();
    assert_eq!(prompt["home"], home.to_string_lossy().as_ref());
    assert_eq!(prompt["setting"], "role-value");
    assert_eq!(prompt["state_dir"], root.to_string_lossy().as_ref());
    drop(web);
    // No subscriber is connected while this turn completes.
    reconnected
        .start_prompt_once(id, "request-2", content("hello"))
        .await
        .unwrap();
    idle(&reconnected, id).await;
    let mut events = reconnected.subscribe_events().await.unwrap();
    let mut delivered = std::collections::HashSet::new();
    timeout(Duration::from_secs(10), async {
        while delivered.is_empty() {
            if let Event::Completed {
                id: event_id,
                completion,
            } = events.recv().await.unwrap()
            {
                assert_eq!(completion.session.thread_id, *id);
                delivered.insert(completion.turn_id);
                reconnected.acknowledge_completion(&event_id).await.unwrap();
            }
        }
    })
    .await
    .unwrap();
    assert!(delivered.contains("request-2"));
    drop(events);
    stop(&mut daemon).await;
    assert!(!socket.exists());
    let mut daemon = spawn(root, &socket);
    ready(&reconnected).await;
    assert_ne!(
        reconnected.health().await.unwrap().instance_id,
        first_instance
    );
    let snapshot = reconnected.snapshot(id).await.unwrap();
    assert_eq!(snapshot.status, "disconnected");
    assert_eq!(snapshot.prompt_receipts.len(), 2);
    assert_eq!(snapshot.execution_environment.unwrap().home, home);
    let mut events = reconnected.subscribe_events().await.unwrap();
    assert!(
        timeout(Duration::from_millis(150), events.recv())
            .await
            .is_err(),
        "acknowledged completions must not replay"
    );
    reconnected.resume(id).await.unwrap();
    reconnected
        .start_prompt_once(id, "request-1", content("wait"))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.join("lineage-prompts.jsonl"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    stop(&mut daemon).await;
}

#[tokio::test]
async fn private_socket_and_single_store_owner_are_enforced() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let directory = root();
    let root = directory.path();
    let socket = root.join("runtime/acpd/acpd.sock");
    let mut daemon = spawn(root, &socket);
    let client = AcpClient::remote(socket.clone());
    ready(&client).await;
    assert_eq!(std::fs::metadata(&socket).unwrap().mode() & 0o777, 0o600);
    assert_eq!(
        std::fs::metadata(socket.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let mut duplicate = spawn(root, &root.join("other/acpd.sock"));
    assert!(
        !timeout(Duration::from_secs(5), duplicate.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(client.health().await.is_ok());
    let unsafe_parent = root.join("unsafe");
    std::fs::create_dir(&unsafe_parent).unwrap();
    std::fs::set_permissions(&unsafe_parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut unsafe_daemon = spawn(&root.join("other-state"), &unsafe_parent.join("acpd.sock"));
    assert!(
        !timeout(Duration::from_secs(5), unsafe_daemon.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    stop(&mut daemon).await;
}

#[tokio::test]
async fn connection_probe_is_ephemeral_and_unknown_session_errors_are_classified() {
    let directory = root();
    let root = directory.path();
    std::fs::write(root.join("aow-acp.json"), settings()).unwrap();
    let socket = root.join("runtime/acpd/acpd.sock");
    let mut daemon = spawn(root, &socket);
    let client = AcpClient::remote(socket.clone());
    ready(&client).await;
    client
        .probe(
            "test",
            root.into(),
            ExecutionEnvironment {
                home: root.into(),
                env: Default::default(),
            },
        )
        .await
        .unwrap();
    assert!(client.sessions(None).await.unwrap().is_empty());
    let error = client.snapshot("missing").await.unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<aow_acpd_client::RpcError>()
            .unwrap()
            .code,
        "not_found"
    );
    stop(&mut daemon).await;
    assert!(client.health().await.is_err());
}

#[tokio::test]
async fn permissions_files_and_workspace_cleanup_work_without_a_web_host() {
    let directory = root();
    let root = directory.path();
    std::fs::write(
        root.join("aow-acp.json"),
        settings().replace("lineage-agent.mjs", "agent.mjs"),
    )
    .unwrap();
    std::fs::write(root.join("source.txt"), "first\nsecond\nthird\n").unwrap();
    let socket = root.join("runtime/acpd/acpd.sock");
    let mut daemon = spawn(root, &socket);
    let client = AcpClient::remote(socket);
    ready(&client).await;
    let mut changes = client.subscribe_sync().await.unwrap();
    let session = client.prepare_session("test", root.into()).await.unwrap();
    let id = &session.thread_id;
    client.resume(id).await.unwrap();
    client
        .start_prompt(id, content("permission"))
        .await
        .unwrap();
    let permission = timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = client.snapshot(id).await.unwrap();
            if let Some(permission) = snapshot.permissions.first() {
                break permission.id.clone();
            }
            sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let reconnected = client.clone();
    reconnected
        .answer(
            id,
            &permission,
            json!({"outcome":{"outcome":"selected", "optionId":"allow"}}),
        )
        .await
        .unwrap();
    idle(&client, id).await;
    client.start_prompt(id, content("files")).await.unwrap();
    idle(&client, id).await;
    assert_eq!(
        std::fs::read_to_string(root.join("written.txt")).unwrap(),
        "second"
    );
    assert!(
        timeout(Duration::from_secs(2), changes.recv())
            .await
            .unwrap()
            .is_ok()
    );
    let overview = client.overview(id).await.unwrap();
    assert!(matches!(
        client
            .overview_updates(id, &overview.sync_generation, overview.revision)
            .await
            .unwrap(),
        aow_acp::SessionUpdates::Unchanged
    ));
    client
        .set_workspace_removing(root.to_str().unwrap(), true)
        .await
        .unwrap();
    assert!(client.prepare_session("test", root.into()).await.is_err());
    assert_eq!(
        client
            .delete_workspace(root.to_str().unwrap())
            .await
            .unwrap(),
        1
    );
    assert!(client.sessions(None).await.unwrap().is_empty());
    assert!(
        root.join("source.txt").exists(),
        "runtime cleanup must not remove the workspace"
    );
    client.recover_workspace_removals().await.unwrap();
    assert!(
        client.prepare_session("test", root.into()).await.is_ok(),
        "a replacement control plane releases abandoned removal reservations"
    );
    stop(&mut daemon).await;
}
