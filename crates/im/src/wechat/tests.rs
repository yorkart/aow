use super::*;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde_json::Value;
use std::{collections::VecDeque, os::unix::fs::PermissionsExt};

#[derive(Default)]
struct Mock {
    sent: Mutex<Vec<Value>>,
    responses: Mutex<VecDeque<(StatusCode, Value)>>,
    updates: Mutex<VecDeque<Value>>,
    cursors: Mutex<Vec<String>>,
}

fn headers(headers: &HeaderMap) {
    assert_eq!(headers["authorization"], "Bearer private-bot-token");
    assert_eq!(headers["authorizationtype"], "ilink_bot_token");
    assert_eq!(headers["ilink-app-id"], "bot");
    assert_eq!(headers["ilink-app-clientversion"], "132105");
    use base64::{Engine, engine::general_purpose::STANDARD};
    let uin = STANDARD.decode(headers["x-wechat-uin"].as_bytes()).unwrap();
    assert!(String::from_utf8(uin).unwrap().parse::<u32>().is_ok());
}

async fn send(
    State(state): State<Arc<Mock>>,
    request_headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    headers(&request_headers);
    assert_eq!(body["base_info"]["channel_version"], "2.4.9");
    state.sent.lock().unwrap().push(body);
    let (status, value) = state
        .responses
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or((StatusCode::OK, json!({"ret":0})));
    (status, Json(value))
}

async fn updates(
    State(state): State<Arc<Mock>>,
    request_headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    headers(&request_headers);
    state
        .cursors
        .lock()
        .unwrap()
        .push(body["get_updates_buf"].as_str().unwrap().into());
    Json(
        state
            .updates
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(json!({"ret":0,"get_updates_buf":"next","msgs":[]})),
    )
}

pub(super) fn credentials() -> Credentials {
    Credentials {
        account_id: "bot-id".into(),
        user_id: "owner-id".into(),
        base_url: api::BASE_URL.into(),
        bot_token: "private-bot-token".into(),
        binding_id: "00000000-0000-4000-8000-000000000001".into(),
    }
}

struct Fixture {
    client: WechatClient,
    mock: Arc<Mock>,
    server: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.client.stop();
        self.server.abort();
    }
}

async fn fixture(root: Option<&Path>) -> Fixture {
    let mock = Arc::new(Mock::default());
    let router = Router::new()
        .route("/ilink/bot/sendmessage", post(send))
        .route("/ilink/bot/getupdates", post(updates))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut client = WechatClient::new(credentials(), root).unwrap();
    let inner = Arc::get_mut(&mut client.inner).unwrap();
    inner.credentials.base_url = url;
    inner.api.http = reqwest::Client::builder()
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    Fixture {
        client,
        mock,
        server,
    }
}

fn message() -> Message {
    Message {
        title: "完成检查".into(),
        fields: vec![
            crate::Field {
                label: "Tab".into(),
                value: "查看结果".into(),
                url: Some("https://aow.example.com/aow/tabs/terminal/a%2Fb".into()),
            },
            crate::Field {
                label: "会话".into(),
                value: "检查结果\nSession ID：session-1".into(),
                url: None,
            },
        ],
        body_label: "结论".into(),
        body: format!(
            "已完成检查。\n第二行说明。\n\n```rust\nlet ok = true;\n```\n\n{}",
            "中文 🦀".repeat(8000)
        ),
        markdown: true,
        error: false,
    }
}

#[tokio::test]
async fn sends_once_without_polling_or_context_and_bounds_unicode_preserving_links() {
    let f = fixture(None).await;
    f.client.send(&message(), "direct-delivery").await.unwrap();
    assert!(f.mock.cursors.lock().unwrap().is_empty());
    let sent = f.mock.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    let msg = &sent[0]["msg"];
    assert_eq!(msg["client_id"], "direct-delivery");
    assert_eq!(msg["to_user_id"], "owner-id");
    assert!(msg.get("context_token").is_none());
    assert_eq!(msg["message_type"], 2);
    let text = msg["item_list"][0]["text_item"]["text"].as_str().unwrap();
    assert!(text.chars().count() <= 4000);
    assert!(text.ends_with('…'));
    assert!(text.starts_with(concat!(
        "完成检查\n\n",
        "Tab：查看结果\n\n",
        "https://aow.example.com/aow/tabs/terminal/a%2Fb\n\n",
        "会话：检查结果\n\n",
        "Session ID：session-1\n\n",
        "结论：\n\n",
        "已完成检查。\n第二行说明。\n\n```rust\nlet ok = true;\n```\n\n中文 🦀",
    )));
}

#[tokio::test]
async fn sends_once_when_success_response_omits_status_codes() {
    for response in [json!({}), json!({"message_id":"123456"})] {
        let f = fixture(None).await;
        f.client
            .inner
            .update_session(|session| session.context_token = Some("valid-context".into()))
            .unwrap();
        f.mock
            .responses
            .lock()
            .unwrap()
            .push_back((StatusCode::OK, response));
        f.client.send(&message(), "delivery").await.unwrap();
        let sent = f.mock.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["msg"]["context_token"], "valid-context");
        assert!(f.client.status().context_ready);
    }
}

#[tokio::test]
async fn explicit_stale_context_retries_once_with_same_id_and_persists_eviction() {
    for rejection in [
        json!({"ret":-2,"errmsg":"prepare failed"}),
        json!({"ret":-14}),
        json!({"ret":0,"errcode":-2,"errmsg":"Unknown Error"}),
    ] {
        let root = tempfile::tempdir().unwrap();
        let f = fixture(Some(root.path())).await;
        f.client
            .inner
            .update_session(|session| session.context_token = Some("expired-context".into()))
            .unwrap();
        f.mock
            .responses
            .lock()
            .unwrap()
            .push_back((StatusCode::OK, rejection));
        f.client.send(&message(), "same-id").await.unwrap();
        let sent = f.mock.sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0]["msg"]["context_token"], "expired-context");
        assert!(sent[1]["msg"].get("context_token").is_none());
        let mut first = sent[0].clone();
        first["msg"]
            .as_object_mut()
            .unwrap()
            .remove("context_token");
        assert_eq!(first, sent[1]);
        let restored = WechatClient::new(credentials(), Some(root.path())).unwrap();
        assert!(!restored.status().context_ready);
        assert_eq!(
            std::fs::metadata(restored.inner.path.as_ref().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[tokio::test]
async fn no_blind_retry_on_http_errors_malformed_success_or_unrelated_rejections() {
    for (status, response) in [
        (StatusCode::INTERNAL_SERVER_ERROR, json!({"ret":-14})),
        (StatusCode::OK, json!(null)),
        (StatusCode::OK, json!([])),
        (StatusCode::OK, json!("private-bot-token")),
        (StatusCode::OK, json!({"ret":null})),
        (StatusCode::OK, json!({"ret":"0"})),
        (StatusCode::OK, json!({"ret":0,"errcode":"secret-context"})),
        (StatusCode::OK, json!({"errcode":false})),
        (StatusCode::OK, json!({"ret":1})),
        (StatusCode::OK, json!({"errcode":1})),
        (StatusCode::OK, json!({"ret":0,"errcode":1})),
        (
            StatusCode::OK,
            json!({"ret":-2,"errmsg":"some unrelated error"}),
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"ret":429,"errmsg":"private-bot-token"}),
        ),
    ] {
        let f = fixture(None).await;
        f.client
            .inner
            .update_session(|session| session.context_token = Some("secret-context".into()))
            .unwrap();
        f.mock
            .responses
            .lock()
            .unwrap()
            .push_back((status, response));
        let error = f
            .client
            .send(&message(), "delivery")
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("private-bot-token"));
        assert!(!error.contains("secret-context"));
        assert_eq!(f.mock.sent.lock().unwrap().len(), 1);
    }
    let f = fixture(None).await;
    f.mock
        .responses
        .lock()
        .unwrap()
        .extend(vec![(StatusCode::OK, json!({"ret":-14})); 3]);
    f.client
        .inner
        .update_session(|session| session.context_token = Some("stale".into()))
        .unwrap();
    assert!(f.client.send(&message(), "delivery").await.is_err());
    assert_eq!(f.mock.sent.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn polling_without_status_codes_saves_only_owner_private_context_and_cursor() {
    let root = tempfile::tempdir().unwrap();
    let f = fixture(Some(root.path())).await;
    f.mock.updates.lock().unwrap().push_back(json!({"get_updates_buf":"cursor-1","msgs":[
        {"from_user_id":"owner-id","message_type":1,"context_token":"owner-context"},
        {"from_user_id":"other-id","message_type":1,"context_token":"attacker-context"},
        {"from_user_id":"owner-id","message_type":1,"group_id":"room","context_token":"group-context"},
        {"from_user_id":"owner-id","message_type":2,"context_token":"bot-context"}
    ]}));
    f.client.inner.receive().await.unwrap();
    f.client.send(&message(), "delivery").await.unwrap();
    assert_eq!(
        f.mock.sent.lock().unwrap()[0]["msg"]["context_token"],
        "owner-context"
    );
    let restored = WechatClient::new(credentials(), Some(root.path())).unwrap();
    {
        let session = restored.inner.session.lock().unwrap();
        assert_eq!(session.cursor, "cursor-1");
        assert_eq!(session.context_token.as_deref(), Some("owner-context"));
    }
    f.mock.updates.lock().unwrap().push_back(json!({}));
    f.client.inner.receive().await.unwrap();
    {
        let session = f.client.inner.session.lock().unwrap();
        assert_eq!(session.cursor, "cursor-1");
        assert_eq!(session.context_token.as_deref(), Some("owner-context"));
    }
    assert_eq!(*f.mock.cursors.lock().unwrap(), ["", "cursor-1"]);
    assert!(
        !serde_json::to_string(&f.client.status())
            .unwrap()
            .contains("owner-context")
    );
}

#[test]
fn rejects_untrusted_credential_destinations_and_unsafe_session_paths() {
    for base in [
        "http://ilinkai.weixin.qq.com",
        "https://evil.example",
        "https://ilinkai.weixin.qq.com.evil.example",
        "https://ilinkai.weixin.qq.com:8443",
        "https://secret@ilinkai.weixin.qq.com",
        "https://ilinkai.weixin.qq.com/?token=secret",
    ] {
        let mut credentials = credentials();
        credentials.base_url = base.into();
        assert!(WechatClient::new(credentials, None).is_err());
    }
    let mut credentials = credentials();
    credentials.binding_id = "../../escape".into();
    assert!(WechatClient::new(credentials, None).is_err());
}

#[tokio::test]
async fn polling_is_owned_once_and_stopping_it_does_not_disable_direct_send() {
    let root = tempfile::tempdir().unwrap();
    let f = fixture(Some(root.path())).await;
    f.mock.updates.lock().unwrap().push_back(json!({}));
    f.client.inner.status.lock().unwrap().error = Some("previous failure".into());
    f.client.start();
    f.client.start();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !f.client.status().receiving {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(f.client.status().error.is_none());
    f.client.stop();
    f.client.send(&message(), "after-stop").await.unwrap();
    assert_eq!(f.mock.cursors.lock().unwrap().len(), 1);
    f.client
        .inner
        .update_session(|session| session.context_token = Some("private-context".into()))
        .unwrap();
    let path = f.client.inner.path.as_ref().unwrap();
    assert!(path.exists());
    f.client.retire();
    assert!(!path.exists());
    assert!(f.client.send(&message(), "after-remove").await.is_err());
    assert_eq!(f.mock.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn broken_transport_is_not_retried_and_error_contains_no_secrets() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = WechatClient::new(credentials(), None).unwrap();
    let inner = Arc::get_mut(&mut client.inner).unwrap();
    inner.credentials.base_url = format!("http://{}", listener.local_addr().unwrap());
    inner.api.http = reqwest::Client::builder()
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let accepted = calls.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 16384];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            accepted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Close after receiving the request: remote delivery is uncertain.
        }
    });
    let result = tokio::time::timeout(Duration::from_secs(2), client.send(&message(), "uncertain"))
        .await
        .unwrap();
    server.abort();
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert!(
        !result
            .unwrap_err()
            .to_string()
            .contains("private-bot-token")
    );
}
