use anyhow::Result;
use aow_runtime_daemon::{Options, run_with_listener};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const TOKEN: &str = "test-node-token-0123456789abcdef0123456789abcdef";
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Running {
    url: String,
    client: Client,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<()>>>,
}
impl Running {
    async fn start(root: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let options = Options {
            state_dir: root.into(),
            listen: address,
            token: TOKEN.into(),
            allowed_origins: Vec::new(),
            vt_node: None,
            vt_worker: None,
        };
        let mut task = tokio::spawn(run_with_listener(options, listener, async {
            let _ = stopped.await;
        }));
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let url = format!("http://{address}");
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if task.is_finished() {
                    panic!("node exited before ready: {:?}", (&mut task).await);
                }
                if client
                    .get(format!("{url}/v1/health"))
                    .bearer_auth(TOKEN)
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Self {
            url,
            client,
            stop: Some(stop),
            task: Some(task),
        }
    }
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(TOKEN)
    }
    async fn post(&self, path: &str, value: Value) -> Value {
        self.request(reqwest::Method::POST, path)
            .json(&value)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn get(&self, path: &str) -> Value {
        self.request(reqwest::Method::GET, path)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn socket(&self, path: &str) -> Socket {
        let mut request = format!("{}{path}", self.url.replace("http:", "ws:"))
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
        let (socket, response) = connect_async(request).await.unwrap();
        if path.contains("/acp") {
            assert!(response.headers().contains_key("acp-connection-id"));
        }
        socket
    }
    async fn stop(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(15), self.task.take().unwrap())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        // Sending shutdown also reaps children if an assertion fails.
        self.stop.take();
    }
}
async fn receive(socket: &mut Socket) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                return serde_json::from_str(&text).unwrap();
            }
        }
    })
    .await
    .unwrap()
}
async fn send(socket: &mut Socket, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}
async fn initialize(socket: &mut Socket) -> Value {
    send(socket, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}})).await;
    receive(socket).await
}
fn pid_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
async fn assert_reaped(pid: u32) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while pid_exists(pid) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("child {pid} is still alive"));
}
fn configure(root: &Path) {
    let adapter = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent.mjs");
    std::fs::write(
        root.join("aow-acp.json"),
        json!({"agent_servers":{"fixture":{
        "type":"custom","command":"node","args":[adapter]}}})
        .to_string(),
    )
    .unwrap();
    let path: Vec<_> = std::env::split_paths(&std::env::var_os("PATH").unwrap()).collect();
    std::fs::write(
        root.join("aow-settings.json"),
        json!({"execution_path":path}).to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn native_acp_roundtrip_authentication_and_connection_cleanup() {
    let root = tempfile::tempdir().unwrap();
    configure(root.path());
    let node = Running::start(root.path()).await;
    assert_eq!(
        node.client
            .get(format!("{}/v1/health", node.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        node.request(reqwest::Method::GET, "/v1/health")
            .header("Origin", "https://untrusted.example")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let health = node.get("/v1/health").await;
    assert_eq!(health["pid"], std::process::id());
    assert_eq!(node.get("/v1/terminal/health").await["pid"], health["pid"]);
    assert_eq!(
        node.get("/v1/acp/agents").await["agents"][0]["id"],
        "fixture"
    );
    assert_eq!(
        node.request(reqwest::Method::POST, "/v1/rpc")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let ws_url = format!(
        "{}/acp?agent_id=fixture&cwd=/",
        node.url.replace("http:", "ws:")
    );
    let error = connect_async(ws_url).await.unwrap_err();
    assert!(
        matches!(error, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == 401)
    );
    assert_eq!(node.get("/v1/acp/instances").await["instances"], json!([]));
    let home = root.path().join("role-home");
    std::fs::create_dir(&home).unwrap();
    let spec = json!({"agent_id":"fixture","cwd":root.path(), "execution_environment":{"home":home,"env":{"AOW_TEST_NODE_VALUE":"isolated"}}});
    let response = node
        .request(reqwest::Method::PUT, "/v1/acp/instances/one")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let pid = response.json::<Value>().await.unwrap()["pid"]
        .as_u64()
        .unwrap() as u32;
    let same = node
        .request(reqwest::Method::PUT, "/v1/acp/instances/one")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(same.status(), StatusCode::OK);
    assert_eq!(same.json::<Value>().await.unwrap()["pid"], pid);
    assert_eq!(
        node.request(reqwest::Method::PUT, "/v1/acp/instances/one")
            .json(&json!({"agent_id":"other","cwd":root.path()}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let mut socket = node.socket("/v1/acp/instances/one/acp").await;
    let response = initialize(&mut socket).await;
    let descendant = response["result"]["_meta"]["descendant"].as_u64().unwrap() as u32;
    assert_eq!(response["result"]["_meta"]["home"], home.to_str().unwrap());
    assert_eq!(response["result"]["_meta"]["sentinel"], "isolated");
    assert_eq!(response["id"], 1);
    assert_eq!(
        response["result"]["_meta"]["state_dir"],
        root.path().to_str().unwrap()
    );
    let mut second = format!(
        "{}/v1/acp/instances/one/acp",
        node.url.replace("http:", "ws:")
    )
    .into_client_request()
    .unwrap();
    second
        .headers_mut()
        .insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    assert!(
        matches!(connect_async(second).await.unwrap_err(), tokio_tungstenite::tungstenite::Error::Http(r) if r.status()==409)
    );
    send(&mut socket,json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":root.path(),"mcpServers":[]}})).await;
    assert_eq!(
        receive(&mut socket).await,
        json!({"jsonrpc":"2.0","id":2,"result":{"sessionId":"native-session","_meta":{"preserved":true}}})
    );
    send(&mut socket,json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"sessionId":"native-session","prompt":[{"type":"text","text":"hello"}]}})).await;
    let permission = receive(&mut socket).await;
    assert_eq!(permission["method"], "session/request_permission");
    send(&mut socket,json!({"jsonrpc":"2.0","id":"permission-1","result":{"outcome":{"outcome":"selected","optionId":"allow"}}})).await;
    assert_eq!(receive(&mut socket).await["method"], "session/update");
    assert_eq!(
        receive(&mut socket).await,
        json!({"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}})
    );
    send(&mut socket, json!([])).await;
    assert_eq!(receive(&mut socket).await["error"]["code"], -32600);
    socket.send(Message::Text("{".into())).await.unwrap();
    assert_eq!(receive(&mut socket).await["error"]["code"], -32700);
    socket
        .send(Message::Binary(vec![1, 2].into()))
        .await
        .unwrap();
    send(&mut socket,json!({"jsonrpc":"2.0","id":"extension","method":"_custom/echo","params":{"futureField":{"a":[1,true,"a\nb"]}}})).await;
    assert_eq!(
        receive(&mut socket).await["result"],
        json!({"futureField":{"a":[1,true,"a\nb"]}})
    );
    socket.close(None).await.unwrap();
    assert_reaped(pid).await;
    assert_reaped(descendant).await;
    assert_eq!(
        node.request(reqwest::Method::DELETE, "/v1/acp/instances/one")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(node.get("/v1/acp/instances").await["instances"], json!([]));
    node.stop().await;
}

#[tokio::test]
async fn terminal_replay_resize_and_unified_shutdown_reap_children() {
    let root = tempfile::tempdir().unwrap();
    configure(root.path());
    let node = Running::start(root.path()).await;
    let spec = json!({"cwd":root.path(),"shell":"/bin/sh","arguments":["-c","printf '%s' \"$$\" > \"$AOW_TEST_PID_FILE\"; printf NODE_TERMINAL; exec /bin/cat"],
        "environment":{"AOW_TEST_PID_FILE":root.path().join("terminal.pid")},"rows":24,"cols":80});
    assert_eq!(
        node.request(reqwest::Method::PUT, "/v1/terminal/runtimes/one")
            .json(&spec)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
    let mut terminal = node.socket("/v1/terminal/runtimes/one/attach").await;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&bytes).contains("NODE_TERMINAL") {
            if let Message::Binary(chunk) = terminal.next().await.unwrap().unwrap() {
                bytes.extend_from_slice(&chunk);
            }
        }
    })
    .await
    .unwrap();
    terminal
        .send(Message::Text(
            json!({"type":"resize","rows":35,"cols":100})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    terminal
        .send(Message::Binary(b"remote-input\n".to_vec().into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let runtime = node.get("/v1/terminal/runtimes/one").await;
            if runtime["cols"] == 100 {
                assert_eq!(runtime["rows"], 35);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    terminal.close(None).await.unwrap();
    let terminal_pid: u32 = std::fs::read_to_string(root.path().join("terminal.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(pid_exists(terminal_pid));
    let mut replay = node.socket("/v1/terminal/runtimes/one/attach").await;
    let mut replayed = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&replayed).contains("remote-input") {
            if let Message::Binary(chunk) = replay.next().await.unwrap().unwrap() {
                replayed.extend_from_slice(&chunk);
            }
        }
    })
    .await
    .unwrap();
    let path = format!("/acp?agent_id=fixture&cwd={}", root.path().display());
    let mut acp = node.socket(&path).await;
    let response = initialize(&mut acp).await;
    let pid = response["result"]["_meta"]["pid"].as_u64().unwrap() as u32;
    let descendant = response["result"]["_meta"]["descendant"].as_u64().unwrap() as u32;
    node.stop().await;
    assert_reaped(pid).await;
    assert_reaped(descendant).await;
    assert_reaped(terminal_pid).await;
}

#[tokio::test]
async fn node_identity_settings_and_exclusive_ownership_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    configure(root.path());
    let first = Running::start(root.path()).await;
    let before = first.get("/v1/health").await;
    let settings = first.get("/v1/acp/settings").await;
    let content = format!(
        "// preserved comment\n{}",
        settings["content"].as_str().unwrap()
    );
    let response = first
        .request(reqwest::Method::PUT, "/v1/acp/settings")
        .json(&json!({"revision":settings["revision"],"content":content}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert!(
        aow_acpd::AcpRuntime::open(root.path().into())
            .await
            .is_err()
    );
    first.stop().await;
    let second = Running::start(root.path()).await;
    let after = second.get("/v1/health").await;
    assert_eq!(after["node_id"], before["node_id"]);
    assert_ne!(after["instance_id"], before["instance_id"]);
    assert_eq!(second.get("/v1/acp/settings").await["content"], content);
    second.stop().await;
}

#[tokio::test]
async fn agent_discovery_and_launch_preparation_use_the_node_environment() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("node-tools");
    std::fs::create_dir(&tools).unwrap();
    let executable = tools.join("node-only-agent");
    std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        root.path().join("aow-settings.json"),
        json!({"execution_path":[tools]}).to_string(),
    )
    .unwrap();
    let node = Running::start(root.path()).await;
    let discovered = node.get("/v1/environment/discovered-path").await;
    assert!(!discovered.as_array().unwrap().is_empty());
    assert_ne!(discovered, json!([tools]));
    let profile = json!({"id":"node-profile","agent_type":"pi","display_name":"Node Pi","command":"node-only-agent","args":[],"env":{}});
    let agents: Value = node
        .request(reqwest::Method::POST, "/v1/agents/discover")
        .json(&json!({"profiles":[profile],"default_agent_id":"node-profile"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(agents[0]["executable"], executable.to_str().unwrap());
    assert_eq!(agents[0]["available"], true);
    assert_eq!(agents[0]["is_default"], true);
    let launch: Value = node
        .request(reqwest::Method::POST, "/v1/agents/resolve")
        .json(&json!({"profile":profile}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(launch["env"]["PATH"], tools.to_str().unwrap());
    let prepared: Value = node
        .request(reqwest::Method::POST, "/v1/agents/prepare-terminal")
        .json(&json!({"launch":launch,"pane_id":"node-pane","cwd":root.path()}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        prepared["env"]["AOW_STATE_DIR"],
        root.path().to_str().unwrap()
    );
    let binding = PathBuf::from(prepared["env"]["AOW_PI_BINDING"].as_str().unwrap());
    assert!(binding.starts_with(root.path().join("data/agents/pi")));
    assert!(
        prepared["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg.as_str().is_some_and(|arg| arg.contains("pi")))
    );
    node.stop().await;
}

#[tokio::test]
async fn native_sessions_are_scoped_to_node_owned_homes() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("node-home");
    std::fs::create_dir(&home).unwrap();
    let node = Running::start(root.path()).await;
    let scope = json!({"cwd":root.path(),"agent":"claude","home":home});
    let response = node
        .request(reqwest::Method::POST, "/v1/agent-sessions/list")
        .json(&scope)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(response.json::<Value>().await.unwrap(), json!([]));
    let mut missing = scope.clone();
    missing["session_id"] = json!("unknown-session");
    assert_eq!(
        node.request(reqwest::Method::POST, "/v1/agent-sessions/snapshot")
            .json(&missing)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let mut invalid = scope;
    invalid["home"] = json!("relative-home");
    assert_eq!(
        node.request(reqwest::Method::POST, "/v1/agent-sessions/list")
            .json(&invalid)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    node.stop().await;
}

#[tokio::test]
async fn native_session_reference_reads_after_node_restart_and_worktree_removal() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("node-home");
    let workspace = root.path().join("worktree");
    let history = home.join(".claude/projects/project");
    std::fs::create_dir_all(&history).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let transcript = history.join("session.jsonl");
    std::fs::write(&transcript,format!("{}\n{}\n",json!({"type":"user","uuid":"user-1","sessionId":"native","cwd":workspace,"message":{"role":"user","content":"Read on execution node"}}),json!({"type":"assistant","uuid":"assistant-1","parentUuid":"user-1","sessionId":"native","cwd":workspace,"message":{"role":"assistant","content":[{"type":"text","text":"Node response"}]}}))).unwrap();
    let node = Running::start(root.path()).await;
    let sessions: Value = node
        .request(reqwest::Method::POST, "/v1/agent-sessions/list")
        .json(&json!({"cwd":workspace,"agent":"claude","home":home}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sessions.as_array().unwrap().len(), 1, "{sessions}");
    let reference = sessions[0]["reference"].clone();
    assert!(reference["handle"].is_string());
    assert!(!sessions.to_string().contains(transcript.to_str().unwrap()));
    let node_id = node.get("/v1/health").await["node_id"].clone();
    node.stop().await;
    std::fs::remove_dir(&workspace).unwrap();
    let node = Running::start(root.path()).await;
    assert_eq!(node.get("/v1/health").await["node_id"], node_id);
    let snapshot: Value = node
        .request(reqwest::Method::POST, "/v1/agent-sessions/snapshot")
        .json(&json!({"reference":reference}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(snapshot.to_string().contains("Node response"));
    let mut wrong = reference;
    wrong["node_id"] = json!("another-node");
    assert_eq!(
        node.request(reqwest::Method::POST, "/v1/agent-sessions/snapshot")
            .json(&json!({"reference":wrong}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::GONE
    );
    assert_eq!(
        node.client
            .get(format!("{}/v1/agent-events", node.url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    node.stop().await;
}

#[tokio::test]
async fn role_environment_tickets_survive_node_restart_and_replayed_decisions() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source-home");
    std::fs::create_dir_all(source.join(".codex")).unwrap();
    std::fs::write(source.join(".codex/auth.json"), "isolated identity").unwrap();
    let node = Running::start(root.path()).await;
    let spec = json!({"id":"remote-role","home_layout":"codex","identity":{"kind":"local_home","source":source},"skills":[]});
    let token = uuid::Uuid::new_v4().to_string();
    let preparation: Value = node
        .request(reqwest::Method::POST, "/v1/environments/roles/prepare")
        .json(&json!({"token":token,"spec":spec}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let home = PathBuf::from(preparation["home"].as_str().unwrap());
    assert!(home.starts_with(root.path().canonicalize().unwrap()));
    assert_eq!(
        std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
        "isolated identity"
    );
    node.stop().await;
    std::fs::remove_dir_all(source).unwrap();
    let node = Running::start(root.path()).await;
    node.request(reqwest::Method::POST, "/v1/environments/roles/finish")
        .json(&json!({"token":token,"spec":spec,"commit":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let skill = root.path().join("skills/review");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "new skill").unwrap();
    let mut next = spec.clone();
    next["skills"] = json!([skill]);
    let next_token = uuid::Uuid::new_v4().to_string();
    node.request(reqwest::Method::POST, "/v1/environments/roles/prepare")
        .json(&json!({"token":next_token,"spec":next}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    node.request(reqwest::Method::POST, "/v1/environments/roles/finish")
        .json(&json!({"token":next_token,"spec":next,"commit":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    // Replaying an old acknowledged commit must not revert a newer environment.
    node.request(reqwest::Method::POST, "/v1/environments/roles/finish")
        .json(&json!({"token":token,"spec":spec,"commit":true}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join(".codex/skills/review/SKILL.md")).unwrap(),
        "new skill"
    );
    let early = uuid::Uuid::new_v4().to_string();
    let mut rejected = spec.clone();
    rejected["id"] = json!("cancelled-role");
    node.request(reqwest::Method::POST, "/v1/environments/roles/finish")
        .json(&json!({"token":early,"spec":rejected,"commit":false}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        node.request(reqwest::Method::POST, "/v1/environments/roles/prepare")
            .json(&json!({"token":early,"spec":rejected}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    node.stop().await;
}

fn execution_spec(root: &Path, id: &str, wait: bool) -> Value {
    let cwd = root.join("repository");
    std::fs::create_dir_all(&cwd).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&cwd)
            .status()
            .unwrap()
            .success()
    );
    let script = root.join("execution-agent.sh");
    std::fs::write(&script, include_str!("fixtures/execution-agent.sh")).unwrap();
    let mut environment = json!({"CLAUDE_CONFIG_DIR":root.join("native-claude"),"EXECUTION_COUNT_FILE":root.join(format!("{id}.count")),"EXECUTION_PROMPT_FILE":root.join(format!("{id}.prompt"))});
    if wait {
        environment["EXECUTION_WAIT_FILE"] = json!(root.join(format!("{id}.release")));
    }
    json!({"id":id,"repository_path":cwd,"launch":{"executable":"/bin/sh","args":[script],"environment":environment},"input":{"agent":"claude","prompt":"node input","workspace_mode":"existing","workspace_path":cwd,"base_branch":"HEAD","yolo":false},"capacity":{"key":"execution-group","limit":1}})
}
async fn completed_execution(node: &Running, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = node.get(&format!("/v1/executions/{id}")).await;
            if snapshot["terminal"] == true {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn node_execution_is_idempotent_and_keeps_results_after_server_disconnect() {
    let root = tempfile::tempdir().unwrap();
    let node = Running::start(root.path()).await;
    let node_id = node.get("/v1/health").await["node_id"].clone();
    let spec = execution_spec(root.path(), "job-one", false);
    for _ in 0..2 {
        node.request(reqwest::Method::PUT, "/v1/executions/job-one")
            .json(&json!({"node_id":node_id,"spec":spec}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    let snapshot = completed_execution(&node, "job-one").await;
    assert_eq!(
        std::fs::read_to_string(root.path().join("job-one.count")).unwrap(),
        "started\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("job-one.prompt")).unwrap(),
        "node input"
    );
    assert_eq!(
        snapshot["events"].as_array().unwrap().last().unwrap()["event"]["status"],
        "completed",
        "{snapshot}"
    );
    assert_eq!(
        snapshot["result"]["conclusion"], "completed on execution node",
        "{snapshot}"
    );
    let reference = snapshot["native_session"]["reference"].clone();
    assert!(reference["handle"].is_string(), "{snapshot}");
    // The caller can disappear and replay the same journal after node restart.
    node.stop().await;
    let node = Running::start(root.path()).await;
    let recovered = node.get("/v1/executions/job-one?after=1").await;
    assert_eq!(recovered["events"][0]["sequence"], 2);
    assert_eq!(recovered["result"], snapshot["result"]);
    node.request(
        reqwest::Method::DELETE,
        &format!(
            "/v1/executions/job-one?node_id={}",
            node_id.as_str().unwrap()
        ),
    )
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap();
    assert_eq!(
        node.request(reqwest::Method::PUT, "/v1/executions/job-one")
            .json(&json!({"node_id":node_id,"spec":spec}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::GONE
    );
    let shared: Value = node
        .request(reqwest::Method::POST, "/v1/agent-sessions/snapshot")
        .json(&json!({"reference":reference}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(shared.to_string().contains("completed on execution node"));
    node.stop().await;
}

#[tokio::test]
async fn cancellation_before_admission_never_starts_an_execution() {
    let root = tempfile::tempdir().unwrap();
    let node = Running::start(root.path()).await;
    let node_id = node.get("/v1/health").await["node_id"].clone();
    let spec = execution_spec(root.path(), "cancelled-before-submit", false);
    node.request(
        reqwest::Method::POST,
        "/v1/executions/cancelled-before-submit/cancel",
    )
    .json(&json!({"node_id":node_id}))
    .send()
    .await
    .unwrap()
    .error_for_status()
    .unwrap();
    assert_eq!(
        node.request(
            reqwest::Method::PUT,
            "/v1/executions/cancelled-before-submit"
        )
        .json(&json!({"node_id":node_id,"spec":spec}))
        .send()
        .await
        .unwrap()
        .status(),
        StatusCode::GONE
    );
    let result = node.get("/v1/executions/cancelled-before-submit").await;
    assert_eq!(result["events"][0]["event"]["status"], "interrupted");
    assert!(!root.path().join("cancelled-before-submit.count").exists());
    node.stop().await;
}

#[tokio::test]
async fn workspace_transfers_preserve_ranges_versions_and_node_paths() {
    let root = tempfile::tempdir().unwrap();
    let node = Running::start(root.path()).await;
    let file = root.path().join("remote file.txt");
    let url = reqwest::Url::parse_with_params(
        &format!("{}/v1/workspace/raw", node.url),
        &[("path", file.to_str().unwrap())],
    )
    .unwrap();
    let uploaded = node
        .client
        .put(url.clone())
        .bearer_auth(TOKEN)
        .body("hello node")
        .send()
        .await
        .unwrap();
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let version = uploaded.headers()["etag"].clone();
    let read = node
        .client
        .get(url.clone())
        .bearer_auth(TOKEN)
        .header("Range", "bytes=6-9")
        .send()
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(read.headers()["content-range"], "bytes 6-9/10");
    assert_eq!(read.text().await.unwrap(), "node");
    let conflict = node
        .client
        .put(url.clone())
        .bearer_auth(TOKEN)
        .header("If-Match", "\"outdated\"")
        .body("lost")
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::PRECONDITION_FAILED);
    node.client
        .put(url)
        .bearer_auth(TOKEN)
        .header("If-Match", version)
        .body("updated")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let metadata: Value = node
        .post(
            "/v1/workspace/operations",
            json!({"operation":"metadata","arguments":{"path":file}}),
        )
        .await;
    assert_eq!(metadata["size"], 7);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "updated");
    let invalid = node
        .request(reqwest::Method::POST, "/v1/workspace/operations")
        .json(&json!({"operation":"read","arguments":{"path":"relative/file"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let image = node
        .request(reqwest::Method::POST, "/v1/workspace/clipboard")
        .body("invalid image")
        .send()
        .await
        .unwrap();
    assert_eq!(image.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        std::fs::read_dir(root.path().join("cache/clipboard-images"))
            .unwrap()
            .count(),
        0
    );
    node.stop().await;
}

#[tokio::test]
async fn active_execution_reservations_cancel_and_shutdown_on_the_node() {
    let root = tempfile::tempdir().unwrap();
    let node = Running::start(root.path()).await;
    let node_id = node.get("/v1/health").await["node_id"].clone();
    let first = execution_spec(root.path(), "active-one", true);
    node.request(reqwest::Method::PUT, "/v1/executions/active-one")
        .json(&json!({"node_id":node_id,"spec":first}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !root.path().join("active-one.count").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let second = execution_spec(root.path(), "active-two", true);
    let busy = node
        .request(reqwest::Method::PUT, "/v1/executions/active-two")
        .json(&json!({"node_id":node_id,"spec":second}))
        .send()
        .await
        .unwrap();
    assert_eq!(busy.status(), StatusCode::CONFLICT);
    node.post(
        "/v1/executions/active-one/cancel",
        json!({"node_id":node_id}),
    )
    .await;
    let interrupted = completed_execution(&node, "active-one").await;
    assert_eq!(
        interrupted["events"].as_array().unwrap().last().unwrap()["event"]["status"],
        "interrupted"
    );
    node.request(reqwest::Method::PUT, "/v1/executions/active-two")
        .json(&json!({"node_id":node_id,"spec":second}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    node.stop().await;
    let node = Running::start(root.path()).await;
    let interrupted = completed_execution(&node, "active-two").await;
    assert_eq!(
        interrupted["events"].as_array().unwrap().last().unwrap()["event"]["status"],
        "interrupted"
    );
    node.stop().await;
}
