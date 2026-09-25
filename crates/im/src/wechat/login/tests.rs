use super::*;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct Mock {
    tokens: Mutex<Vec<Value>>,
    responses: Mutex<VecDeque<Value>>,
    codes: Mutex<Vec<Option<String>>>,
    block: AtomicBool,
    arrived: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

async fn qr(
    State(state): State<Arc<Mock>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(query["bot_type"], "3");
    assert!(headers.get("authorization").is_none());
    assert!(body.get("base_info").is_none());
    state
        .tokens
        .lock()
        .unwrap()
        .push(body["local_token_list"].clone());
    Json(json!({"qrcode":"opaque-qr", "qrcode_img_content":"https://weixin.qq.com/test-login"}))
}

async fn status(
    State(state): State<Arc<Mock>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Json<Value> {
    assert!(headers.get("authorization").is_none());
    assert_eq!(query["qrcode"], "opaque-qr");
    state
        .codes
        .lock()
        .unwrap()
        .push(query.get("verify_code").cloned());
    if state.block.load(Ordering::Relaxed) {
        state.arrived.notify_one();
        state.release.notified().await;
    }
    Json(
        state
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(json!({"status":"wait"})),
    )
}

struct Fixture {
    login: Arc<LoginManager>,
    mock: Arc<Mock>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn fixture() -> Fixture {
    let mock = Arc::new(Mock::default());
    let router = Router::new()
        .route("/ilink/bot/get_bot_qrcode", post(qr))
        .route("/ilink/bot/get_qrcode_status", get(status))
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut login = LoginManager::new().unwrap();
    login.base_url = format!("http://{}", listener.local_addr().unwrap());
    login.api.http = reqwest::Client::builder().no_proxy().build().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Fixture {
        login: Arc::new(login),
        mock,
        server,
    }
}
fn confirmed() -> Value {
    json!({"status":"confirmed", "bot_token":"new-private-token", "ilink_bot_id":"bot", "ilink_user_id":"scanner", "baseurl":BASE_URL})
}

#[tokio::test]
async fn qr_verification_binds_scanner_and_never_exposes_credentials() {
    let f = fixture().await;
    let login = f.login.start(None).await.unwrap();
    assert!(
        login
            .qr_image
            .as_ref()
            .unwrap()
            .starts_with("data:image/svg+xml;base64,")
    );
    assert_eq!(f.mock.tokens.lock().unwrap()[0], json!([]));
    f.mock.responses.lock().unwrap().extend([
        json!({"status":"scaned"}),
        json!({"status":"need_verifycode"}),
        confirmed(),
    ]);
    assert_eq!(
        f.login
            .poll(&login.id, None, |_| panic!())
            .await
            .unwrap()
            .status,
        "scaned"
    );
    assert_eq!(
        f.login
            .poll(&login.id, None, |_| panic!())
            .await
            .unwrap()
            .status,
        "need_verifycode"
    );
    let view = f
        .login
        .poll(&login.id, Some("123456"), |credentials| {
            assert_eq!(credentials.user_id, "scanner");
            assert_eq!(credentials.bot_token, "new-private-token");
            credentials.validate()
        })
        .await
        .unwrap();
    assert_eq!(view.status, "confirmed");
    assert!(view.qr_image.is_none());
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("new-private-token")
    );
    assert_eq!(
        f.mock.codes.lock().unwrap().last().unwrap().as_deref(),
        Some("123456")
    );
    // A retried HTTP poll after a successful commit must not bind twice.
    f.login
        .poll(&login.id, None, |_| panic!("duplicate commit"))
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelled_or_replaced_inflight_login_cannot_commit() {
    for replace in [false, true] {
        let f = fixture().await;
        let view = f.login.start(None).await.unwrap();
        f.mock.responses.lock().unwrap().push_back(confirmed());
        f.mock.block.store(true, Ordering::Relaxed);
        let manager = f.login.clone();
        let id = view.id.clone();
        let poll = tokio::spawn(async move {
            manager
                .poll(&id, None, |_| panic!("cancelled login committed"))
                .await
        });
        f.mock.arrived.notified().await;
        if replace {
            f.login.start(None).await.unwrap();
        } else {
            f.login.cancel(&view.id);
        }
        f.mock.release.notify_one();
        assert!(poll.await.unwrap().is_err());
    }
}

#[tokio::test]
async fn expired_blocked_untrusted_and_missing_bindings_never_commit() {
    let f = fixture().await;
    for response in [
        json!({"status":"expired"}),
        json!({"status":"verify_code_blocked"}),
        json!({"status":"scaned_but_redirect","redirect_host":"evil.example"}),
        json!({"status":"binded_redirect"}),
        json!({"status":"confirmed"}),
    ] {
        let view = f.login.start(None).await.unwrap();
        f.mock.responses.lock().unwrap().push_back(response);
        let _ = f
            .login
            .poll(&view.id, None, |_| panic!("invalid login committed"))
            .await;
    }
    let view = f.login.start(None).await.unwrap();
    f.login.active.lock().unwrap().as_mut().unwrap().started =
        Instant::now() - Duration::from_secs(301);
    assert!(f.login.poll(&view.id, None, |_| panic!()).await.is_err());
}

#[tokio::test]
async fn already_bound_restores_only_existing_local_credentials() {
    let f = fixture().await;
    let previous = crate::wechat::tests::credentials();
    let view = f.login.start(Some(previous.clone())).await.unwrap();
    assert_eq!(
        f.mock.tokens.lock().unwrap()[0],
        json!(["private-bot-token"])
    );
    f.mock
        .responses
        .lock()
        .unwrap()
        .push_back(json!({"status":"binded_redirect"}));
    assert_eq!(
        f.login
            .poll(&view.id, None, |credentials| {
                assert!(credentials == previous);
                Ok(())
            })
            .await
            .unwrap()
            .status,
        "confirmed"
    );

    let view = f
        .login
        .start(Some(crate::wechat::tests::credentials()))
        .await
        .unwrap();
    f.mock
        .responses
        .lock()
        .unwrap()
        .push_back(json!({"status":"binded_redirect", "ilink_bot_id":"another-bot"}));
    assert!(
        f.login
            .poll(&view.id, None, |_| panic!("restored a different account"))
            .await
            .is_err()
    );
}
