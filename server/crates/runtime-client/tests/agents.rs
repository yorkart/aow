use aow_runtime_client::{RuntimeClient, agents::*, testing::TestRuntime};
use aow_runtime_contract::terminal::TerminalRuntimeSpec;
use std::{collections::BTreeMap, time::Duration};

async fn fixture() -> (tempfile::TempDir, TestRuntime, RuntimeClient) {
    let root = tempfile::tempdir().unwrap();
    let node = TestRuntime::with_worker(
        &root.path().join("node.json"),
        Some("node".into()),
        Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../runtime/vt-worker/dist/vt-worker.mjs"),
        ),
    )
    .await;
    let client = RuntimeClient::new(node.endpoint.clone());
    (root, node, client)
}

#[tokio::test]
async fn agent_preparation_and_capabilities_are_owned_by_the_node() {
    let (root, node, client) = fixture().await;
    let capabilities = client.agent_capabilities().await.unwrap();
    assert!(capabilities.iter().any(|agent| agent.id == "codex"
        && agent.interactive
        && agent.automation
        && agent.sessions));
    let launch = client
        .resolve_agent(&ResolveAgent {
            profile: AgentProfile {
                id: "configured".into(),
                agent_type: Some(AgentType::Codex),
                display_name: "Work".into(),
                command: "/bin/sh".into(),
                args: vec![],
                env: BTreeMap::from([("AOW_STATE_DIR".into(), "/wrong-node".into())]),
            },
        })
        .await
        .unwrap();
    assert_ne!(launch.env["AOW_STATE_DIR"], "/wrong-node");
    let request = PrepareTerminal {
        launch,
        pane_id: "pane".into(),
        cwd: root.path().into(),
        resume_session_id: None,
        managed: true,
    };
    let first = client.prepare_agent_terminal(&request).await.unwrap();
    let second = client
        .prepare_agent_terminal(&PrepareTerminal {
            launch: first.clone(),
            ..request
        })
        .await
        .unwrap();
    assert_eq!(first.args, second.args);
    let plan = client
        .prepare_role_session(&PrepareRoleSession {
            launch: AgentLaunch {
                agent_type: AgentType::Hermes,
                ..first
            },
            instruction: "Role instruction".into(),
            prompt: Some("Task".into()),
        })
        .await
        .unwrap();
    assert_eq!(plan.task.as_deref(), Some("Role instruction\n\nTask"));
    assert!(plan.managed);
    assert!(
        client
            .validate_agent_automation(&ValidateAutomation {
                agent: AgentType::Codex,
                arguments: vec!["--resume".into(), "unexpected".into()]
            })
            .await
            .is_err()
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn initialization_survives_a_disconnected_waiter_and_retries_do_not_repeat_input() {
    let (root, node, client) = fixture().await;
    let terminal = client.terminal();
    let ready = root.path().join("ready");
    let input = root.path().join("input");
    terminal.create("pane",&TerminalRuntimeSpec {
        cwd:root.path().to_string_lossy().into_owned(),shell:"/bin/sh".into(),
        arguments:vec!["-c".into(),"stty -echo; while [ ! -f \"$READY\" ]; do sleep 0.02; done; printf 'model · /workspace\\r\\n'; while IFS= read -r line; do printf '%s\\n' \"$line\" >> \"$INPUT\"; done".into()],
        environment:BTreeMap::from([("READY".into(),ready.to_string_lossy().into_owned()),("INPUT".into(),input.to_string_lossy().into_owned())]),environment_remove:vec![],rows:24,cols:80
    }).await.unwrap();
    let request = TerminalAgentRequest {
        request_id: "initialize-pane".into(),
        operation: TerminalAgentOperation::Initialize {
            agent: "codex".into(),
            task: Some("Exactly once".into()),
            timeout_seconds: 10,
        },
    };
    let waiter = tokio::spawn({
        let terminal = terminal.clone();
        let request = request.clone();
        async move { terminal.run_agent_operation("pane", &request).await }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while terminal
            .agent_operation_status("pane", &request.request_id)
            .await
            .is_err()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    waiter.abort();
    let _ = waiter.await;
    std::fs::write(ready, "go").unwrap();
    // A newly created transport can recover the same operation after its caller went away.
    let recovered = RuntimeClient::new(node.endpoint.clone())
        .terminal()
        .run_agent_operation("pane", &request)
        .await
        .unwrap();
    assert!(
        recovered.finished && recovered.task_submitted && recovered.error.is_none(),
        "{recovered:?}"
    );
    let retry = terminal.agent_operation("pane", &request).await.unwrap();
    assert!(retry.finished && retry.task_submitted);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !input.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(input).unwrap().lines().count(), 1);
    let conflict = TerminalAgentRequest {
        operation: TerminalAgentOperation::Submit {
            task: "different".into(),
        },
        ..request
    };
    assert!(
        matches!(terminal.agent_operation("pane",&conflict).await,Err(aow_runtime_client::RuntimeError::HttpStatus{status,..}) if status==http::StatusCode::CONFLICT)
    );
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_initialization_keeps_the_controller_lease_and_reports_process_exit() {
    use aow_runtime_contract::terminal::{
        TerminalAttachClientMessage, TerminalAttachServerMessage, TerminalControlState,
    };
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (root, node, client) = fixture().await;
    let terminal = client.terminal();
    terminal
        .create(
            "pane",
            &TerminalRuntimeSpec {
                cwd: root.path().to_string_lossy().into_owned(),
                shell: "/bin/sh".into(),
                arguments: vec!["-c".into(), "read -r line".into()],
                environment: Default::default(),
                environment_remove: vec![],
                rows: 24,
                cols: 80,
            },
        )
        .await
        .unwrap();
    let mut owner = terminal
        .attach_controlled_with_resume("pane", None, None)
        .await
        .unwrap();
    owner
        .send(Message::Text(
            serde_json::to_string(&TerminalAttachClientMessage::Claim { force: false })
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Text(text) = owner.next().await.unwrap().unwrap()
                && matches!(
                    serde_json::from_str::<TerminalAttachServerMessage>(&text),
                    Ok(TerminalAttachServerMessage::Control {
                        state: TerminalControlState::Claimed
                    })
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let blocked = terminal
        .run_agent_operation(
            "pane",
            &TerminalAgentRequest {
                request_id: "blocked".into(),
                operation: TerminalAgentOperation::Submit {
                    task: "must not arrive".into(),
                },
            },
        )
        .await
        .unwrap();
    assert!(
        blocked
            .error
            .as_deref()
            .is_some_and(|error| error.contains("controlled"))
    );
    assert!(!blocked.task_submitted);
    owner.close(None).await.unwrap();
    terminal.delete("pane").await.unwrap();
    terminal
        .create(
            "exited",
            &TerminalRuntimeSpec {
                cwd: root.path().to_string_lossy().into_owned(),
                shell: "/bin/sh".into(),
                arguments: vec!["-c".into(), "exit 9".into()],
                environment: Default::default(),
                environment_remove: vec![],
                rows: 24,
                cols: 80,
            },
        )
        .await
        .unwrap();
    let exited = terminal
        .run_agent_operation(
            "exited",
            &TerminalAgentRequest {
                request_id: "initialize-exited".into(),
                operation: TerminalAgentOperation::Initialize {
                    agent: "codex".into(),
                    task: None,
                    timeout_seconds: 5,
                },
            },
        )
        .await
        .unwrap();
    assert!(exited.error.is_some());
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn transport_retries_keep_the_same_operation_identity() {
    use axum::{
        Json, Router,
        http::StatusCode,
        routing::{get, post},
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    let posted = Arc::new(Mutex::new(Vec::new()));
    let reads = Arc::new(AtomicUsize::new(0));
    let response = |finished| serde_json::json!({"request_id":"same-id","finished":finished,"task_submitted":finished,"error":null});
    let app = Router::new()
        .route(
            "/v1/terminal/runtimes/pane/agent-operation",
            post({
                let posted = posted.clone();
                move |Json(body): Json<serde_json::Value>| {
                    let posted = posted.clone();
                    async move {
                        let mut requests = posted.lock().unwrap();
                        requests.push(body);
                        if requests.len() == 1 {
                            (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(serde_json::json!({"message":"lost admission response"})),
                            )
                        } else {
                            (StatusCode::OK, Json(response(false)))
                        }
                    }
                }
            }),
        )
        .route(
            "/v1/terminal/runtimes/pane/agent-operation/{request_id}",
            get({
                let reads = reads.clone();
                move || {
                    let reads = reads.clone();
                    async move {
                        if reads.fetch_add(1, Ordering::Relaxed) == 0 {
                            (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(serde_json::json!({"message":"temporary status failure"})),
                            )
                        } else {
                            (StatusCode::OK, Json(response(true)))
                        }
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = aow_runtime_client::RuntimeEndpoint::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        "test-token".into(),
    )
    .unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let result = RuntimeClient::new(endpoint)
        .terminal()
        .run_agent_operation(
            "pane",
            &TerminalAgentRequest {
                request_id: "same-id".into(),
                operation: TerminalAgentOperation::Submit {
                    task: "once".into(),
                },
            },
        )
        .await
        .unwrap();
    assert!(result.task_submitted);
    let requests = posted.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    server.abort();
}

#[tokio::test]
async fn forced_takeover_during_native_paste_prevents_the_enter_key() {
    use aow_runtime_contract::terminal::{
        TerminalAttachClientMessage, TerminalAttachServerMessage, TerminalControlState,
    };
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (root, node, client) = fixture().await;
    let terminal = client.terminal();
    let input = root.path().join("raw-input");
    let ready = root.path().join("ready");
    terminal
        .create(
            "pane",
            &TerminalRuntimeSpec {
                cwd: root.path().to_string_lossy().into_owned(),
                shell: "/bin/sh".into(),
                arguments: vec![
                    "-c".into(),
                    "stty raw -echo; printf ready > \"$READY\"; cat > \"$INPUT\"".into(),
                ],
                environment: BTreeMap::from([
                    ("INPUT".into(), input.to_string_lossy().into_owned()),
                    ("READY".into(), ready.to_string_lossy().into_owned()),
                ]),
                environment_remove: vec![],
                rows: 24,
                cols: 80,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !ready.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let mut owner = terminal
        .attach_controlled_with_resume("pane", None, None)
        .await
        .unwrap();
    owner
        .send(Message::Text(
            serde_json::to_string(&TerminalAttachClientMessage::Claim { force: false })
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Text(text) = owner.next().await.unwrap().unwrap()
                && matches!(
                    serde_json::from_str::<TerminalAttachServerMessage>(&text),
                    Ok(TerminalAttachServerMessage::Control {
                        state: TerminalControlState::Claimed
                    })
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    // Prepare the takeover connection before the narrow paste/Enter interval.
    let mut human = terminal
        .attach_controlled_with_resume("pane", None, None)
        .await
        .unwrap();
    let submission = tokio::spawn(async move {
        aow_runtime_client::TerminalClient::submit_agent_on_socket(&mut owner, "feedback").await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read(&input)
                .unwrap_or_default()
                .ends_with(b"\x1b[201~")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    human
        .send(Message::Text(
            serde_json::to_string(&TerminalAttachClientMessage::Claim { force: true })
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), submission)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert_eq!(std::fs::read(input).unwrap(), b"\x1b[200~feedback\x1b[201~");
    node.shutdown().await.unwrap();
}
