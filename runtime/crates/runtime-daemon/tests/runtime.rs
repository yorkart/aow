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
