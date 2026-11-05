pub(crate) use aow_i18n::http::{middleware, upgrade};
#[cfg(test)]
use aow_i18n::{Language, http::negotiate};
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn respects_quality_support_and_english_default() {
        for (value, expected) in [
            ("", Language::English),
            ("fr-FR", Language::English),
            ("zh-CN", Language::Chinese),
            ("zh-Hans;q=0.9,en;q=0.4", Language::Chinese),
            ("zh;q=0,en;q=0.4", Language::English),
            ("zh;q=NaN", Language::English),
            ("zh;q=0.1,en-US;q=0.9", Language::English),
        ] {
            assert_eq!(negotiate(value), expected, "{value}");
        }
    }
    #[tokio::test]
    async fn response_localizes_messages_and_preserves_error_contract() {
        use axum::{
            Router,
            body::{Body, to_bytes},
            http::{Request, StatusCode},
            routing::get,
        };
        use tower::ServiceExt;
        let app = Router::new()
            .route(
                "/error",
                get(|| async {
                    crate::HttpError::new(
                        StatusCode::BAD_REQUEST,
                        "path_not_absolute",
                        aow_i18n::text("errors.path_must_be_absolute_2"),
                        Some("relative.txt".into()),
                    )
                }),
            )
            .layer(axum::middleware::from_fn(middleware));
        for (language, expected_language, expected_message) in [
            ("en-US", "en-US", "path must be absolute"),
            ("zh-CN", "zh-CN", "路径必须为绝对路径"),
            ("fr-FR", "en-US", "path must be absolute"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/error")
                        .header("Accept-Language", language)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response.headers()["Content-Language"], expected_language);
            assert_eq!(response.headers()["Vary"], "Accept-Language");
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(error["message"], expected_message);
            assert_eq!(error["code"], "path_not_absolute");
            assert_eq!(error["path"], "relative.txt");
        }
    }
}

#[cfg(test)]
mod websocket_tests {
    use super::*;
    use axum::{Router, extract::WebSocketUpgrade, routing::get};
    use futures_util::StreamExt;
    use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

    #[tokio::test]
    async fn websocket_application_locale_overrides_browser_header_and_survives_upgrade() {
        let app = Router::new()
            .route(
                "/socket",
                get(|ws: WebSocketUpgrade| async {
                    upgrade(ws, move |mut socket| async move {
                        tokio::task::yield_now().await;
                        let _ = socket
                            .send(axum::extract::ws::Message::Text(
                                aow_i18n::text("validation.authentication_expired").into(),
                            ))
                            .await;
                    })
                }),
            )
            .layer(axum::middleware::from_fn(middleware));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for (query, header, expected, tag) in [
            ("?lang=zh-CN", "en-US", "身份验证已过期", "zh-CN"),
            ("?lang=en-US", "zh-CN", "Authentication expired", "en-US"),
            ("?lang=fr-FR", "zh-CN", "Authentication expired", "en-US"),
            ("", "zh-CN", "身份验证已过期", "zh-CN"),
            ("", "fr-FR", "Authentication expired", "en-US"),
        ] {
            let mut request = format!("ws://{address}/socket{query}")
                .into_client_request()
                .unwrap();
            request
                .headers_mut()
                .insert("Accept-Language", header.parse().unwrap());
            let (mut socket, response) = connect_async(request).await.unwrap();
            assert_eq!(response.headers()["Content-Language"], tag);
            let message = tokio::time::timeout(std::time::Duration::from_secs(3), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(message.to_text().unwrap(), expected);
            let _ = socket.close(None).await;
        }
        server.abort();
    }
}
