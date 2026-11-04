use super::*;
use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::HeaderMap,
    routing::{get, post},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct Mock {
    tokens: AtomicUsize,
    owner_calls: AtomicUsize,
    messages: Mutex<Vec<Value>>,
    reject_token: AtomicUsize,
    fail_message: AtomicUsize,
    owner_type: AtomicUsize,
}

pub(super) fn notification(conclusion: &str) -> Message {
    Message {
        title: "AoW·完成检查·Codex·完成".into(),
        fields: vec![
            crate::Field {
                label: "Tab".into(),
                value: "检查".into(),
                url: None,
            },
            crate::Field {
                label: "会话".into(),
                value: "完成检查\nSession ID：session-1".into(),
                url: None,
            },
        ],
        body_label: "本轮结论".into(),
        body: conclusion.into(),
        markdown: true,
        error: false,
    }
}

async fn token(State(state): State<Arc<Mock>>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(body["app_id"], "cli_test");
    assert_eq!(body["app_secret"], "test-secret");
    let count = state.tokens.fetch_add(1, Ordering::SeqCst) + 1;
    Json(json!({"code": 0, "tenant_access_token": format!("t-test-{count}"), "expire":7200}))
}

async fn owner(
    State(state): State<Arc<Mock>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Json<Value> {
    assert_eq!(uri.query(), Some("lang=zh_cn&user_id_type=open_id"));
    assert!(
        headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("Bearer t-test-")
    );
    state.owner_calls.fetch_add(1, Ordering::SeqCst);
    if state.reject_token.swap(0, Ordering::SeqCst) > 0 {
        return Json(json!({"code":99991663}));
    }
    Json(
        json!({"code":0,"data":{"app":{"owner":{"type":state.owner_type.load(Ordering::SeqCst),"owner_id":"ou_owner"}}}}),
    )
}

async fn message(
    State(state): State<Arc<Mock>>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(uri.query(), Some("receive_id_type=open_id"));
    state.messages.lock().await.push(body);
    let code = state.fail_message.swap(0, Ordering::SeqCst);
    let status = if code == 500 {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::OK
    };
    (
        status,
        Json(json!({"code":code,"data":{"message_id":"om_test"}})),
    )
}

struct Fixture {
    client: Arc<FeishuClient>,
    state: Arc<Mock>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

async fn fixture() -> Fixture {
    let state = Arc::new(Mock::default());
    state.owner_type.store(2, Ordering::SeqCst);
    let router = Router::new()
        .route(TOKEN_PATH, post(token))
        .route("/application/v6/applications/me", get(owner))
        .route("/im/v1/messages", post(message))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut client = FeishuClient::new("cli_test".into(), "test-secret".into()).unwrap();
    client.base_url = format!("http://{address}");
    client.http = Client::builder()
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    Fixture {
        client: Arc::new(client),
        state,
        server,
    }
}

#[tokio::test]
async fn concurrent_requests_share_token_and_refresh_near_expiry() {
    let fixture = fixture().await;
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let client = fixture.client.clone();
        tasks.push(tokio::spawn(async move { client.token().await.unwrap() }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), "t-test-1");
    }
    assert_eq!(fixture.state.tokens.load(Ordering::SeqCst), 1);
    fixture
        .client
        .token
        .lock()
        .await
        .as_mut()
        .unwrap()
        .refresh_at = Instant::now();
    assert_eq!(fixture.client.token().await.unwrap(), "t-test-2");
    // A late rejection of an old token must not evict the replacement.
    fixture.client.invalidate_token("t-test-1").await;
    assert_eq!(fixture.client.token().await.unwrap(), "t-test-2");
    assert_eq!(fixture.state.tokens.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn automation_failure_uses_existing_transport_with_failure_card_and_run_link() {
    let fixture = fixture().await;
    let mut event = notification("失败 <at id=all></at> **literal**");
    event.title = "自动化失败 · 检查 <at id=all></at>".into();
    event.error = true;
    event.markdown = false;
    event.body_label = "失败原因".into();
    event.fields = vec![
        crate::Field {
            label: "执行 ID".into(),
            value: "run-1".into(),
            url: None,
        },
        crate::Field {
            label: "执行记录".into(),
            value: "查看执行记录".into(),
            url: Some("https://aow.example.com/aow/tabs/automation/12345678/runs/run-1".into()),
        },
    ];
    fixture
        .client
        .send(&event, "failure-delivery")
        .await
        .unwrap();
    let messages = fixture.state.messages.lock().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["uuid"], "failure-delivery");
    assert_eq!(messages[0]["receive_id"], "ou_owner");
    let card: Value = serde_json::from_str(messages[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(card["header"]["template"], "red");
    assert_eq!(
        card["header"]["title"]["content"],
        "自动化失败 · 检查 <at id=all></at>"
    );
    let fields = &card["body"]["elements"][0]["columns"][0]["elements"];
    assert_eq!(fields[0]["text"]["tag"], "plain_text");
    assert_eq!(fields[0]["text"]["content"], "执行 ID：run-1");
    assert_eq!(
        card["body"]["elements"][2]["text"]["content"],
        "失败 <at id=all></at> **literal**"
    );
    assert!(
        fields[1]["text"]["content"]
            .as_str()
            .unwrap()
            .contains(event.fields[1].url.as_ref().unwrap())
    );
}

#[tokio::test]
async fn resolves_owner_and_recovers_rejected_token_without_losing_message_content() {
    let fixture = fixture().await;
    fixture.state.reject_token.store(1, Ordering::SeqCst);
    let text = "Agent 任务完成\n项目：AoW\nTab：检查\nSession ID：session-1";
    fixture
        .client
        .send(&notification(text), "delivery-1")
        .await
        .unwrap();
    assert_eq!(fixture.state.tokens.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.state.owner_calls.load(Ordering::SeqCst), 2);
    let messages = fixture.state.messages.lock().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["receive_id"], "ou_owner");
    assert_eq!(messages[0]["uuid"], "delivery-1");
    assert_eq!(messages[0]["msg_type"], "interactive");
    let card: Value = serde_json::from_str(messages[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(card["schema"], "2.0");
    assert_eq!(card["body"]["elements"][2]["content"], text);
}

#[tokio::test]
async fn authentication_retry_reuses_delivery_id_and_future_events_are_independent() {
    let fixture = fixture().await;
    fixture.state.fail_message.store(99991663, Ordering::SeqCst);
    fixture
        .client
        .send(&notification("first turn"), "delivery-1")
        .await
        .unwrap();
    fixture
        .client
        .send(&notification("next turn"), "delivery-2")
        .await
        .unwrap();
    let messages = fixture.state.messages.lock().await;
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0], messages[1]);
    assert_ne!(messages[1]["uuid"], messages[2]["uuid"]);
}

#[tokio::test]
async fn rate_limit_retries_once_but_server_failures_do_not_retry() {
    let fixture = fixture().await;
    fixture.state.fail_message.store(230020, Ordering::SeqCst);
    fixture
        .client
        .send(&notification("rate limited"), "delivery-1")
        .await
        .unwrap();
    let messages = fixture.state.messages.lock().await;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], messages[1]);
    drop(messages);
    fixture.state.fail_message.store(500, Ordering::SeqCst);
    assert!(
        fixture
            .client
            .send(&notification("failure"), "delivery-2")
            .await
            .is_err()
    );
    assert_eq!(fixture.state.messages.lock().await.len(), 3);
}

#[tokio::test]
async fn non_member_owner_is_not_used_as_a_recipient() {
    let fixture = fixture().await;
    fixture.state.owner_type.store(1, Ordering::SeqCst);
    assert!(
        fixture
            .client
            .send(&notification("hello"), "delivery")
            .await
            .is_err()
    );
    assert!(fixture.state.messages.lock().await.is_empty());
}

#[tokio::test]
async fn multipart_cards_send_in_order_and_retry_only_the_rejected_part() {
    let fixture = fixture().await;
    fixture.state.fail_message.store(99991663, Ordering::SeqCst);
    let event = notification(&"长结论 🦀\n\n".repeat(5000));
    let expected = cards::messages(&event, "ou_owner", "delivery").unwrap();
    assert!(expected.len() > 1);
    fixture.client.send(&event, "delivery").await.unwrap();
    let messages = fixture.state.messages.lock().await;
    assert_eq!(messages.len(), expected.len() + 1);
    assert_eq!(messages[0], messages[1]);
    assert_eq!(messages[1..], expected);
    assert_eq!(fixture.state.owner_calls.load(Ordering::SeqCst), 1);
}
