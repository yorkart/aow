use super::*;
use axum::{Extension, Router, extract::WebSocketUpgrade, routing::get};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn state(socket: &mut Client, expected: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    if value["state"] == expected {
                        return value;
                    }
                }
                Message::Ping(_) => socket.flush().await.unwrap(),
                message => panic!("unexpected control message: {message:?}"),
            }
        }
    })
    .await
    .unwrap()
}

async fn claim(socket: &mut Client, force: bool) {
    socket
        .send(Message::Text(
            json!({"type":"claim", "force":force}).to_string().into(),
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn sockets_broadcast_takeover_release_on_close_and_revoke_on_logout() {
    let directory = tempfile::tempdir().unwrap();
    crate::auth::write_credentials(directory.path(), "admin", "test-password");
    let mut app = crate::AppState::new(Default::default());
    app.auth = crate::auth::AuthService::persistent(directory.path());
    let controller = app.acp_controls.session("test");
    let shared = controller.clone();
    // Run the real socket loop with the same auth middleware as production.
    let router = Router::new()
        .route(
            "/api/control-test",
            get(
                move |ws: WebSocketUpgrade, access: Extension<crate::auth::SessionAccess>| {
                    let controller = shared.clone();
                    async move {
                        ws.on_upgrade(move |socket| socket::run(socket, controller, Some(access.0)))
                    }
                },
            ),
        )
        .merge(crate::auth::routes())
        .merge(crate::acp::routes())
        .layer(axum::middleware::from_fn_with_state(
            app.clone(),
            crate::auth::require_auth,
        ))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let production_url = format!("ws://{address}/api/acp/sessions/test/control");
    assert!(matches!(connect_async(&production_url).await,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 401));
    let http = reqwest::Client::new();
    let login = http
        .post(format!("http://{address}/api/auth/login"))
        .json(&json!({"username":"admin", "password":"test-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let cookie = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let mut request = production_url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    assert!(matches!(connect_async(request).await,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) if response.status() == 403));

    let mut request = format!("ws://{address}/api/control-test")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    let (mut first, _) = connect_async(request.clone()).await.unwrap();
    claim(&mut first, false).await;
    let original = state(&mut first, "claimed").await;
    let (mut second, _) = connect_async(request.clone()).await.unwrap();
    claim(&mut second, false).await;
    assert!(state(&mut second, "observing").await.get("token").is_none());
    claim(&mut second, true).await;
    let replacement = state(&mut second, "claimed").await;
    assert_ne!(original["token"], replacement["token"]);
    assert!(state(&mut first, "observing").await.get("token").is_none());
    first.close(None).await.unwrap();
    // Closing a superseded attachment cannot release the current controller.
    claim(&mut second, false).await;
    assert_eq!(
        state(&mut second, "claimed").await["token"],
        replacement["token"]
    );
    let (mut observer, _) = connect_async(request).await.unwrap();
    observer
        .send(Message::Text(json!({"type":"observe"}).to_string().into()))
        .await
        .unwrap();
    state(&mut observer, "observing").await;
    second.close(None).await.unwrap();
    state(&mut observer, "observing").await;
    assert!(
        controller.owner.lock().unwrap().is_none(),
        "closing the owner never promotes an observer"
    );
    claim(&mut observer, true).await;
    state(&mut observer, "claimed").await;

    let logout = http
        .post(format!("http://{address}/api/auth/logout"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert!(logout.status().is_success());
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match observer.next().await.unwrap().unwrap() {
                Message::Close(_) => break,
                Message::Ping(_) => observer.flush().await.unwrap(),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(controller.owner.lock().unwrap().is_none());
    server.abort();
}
