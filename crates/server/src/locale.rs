//! Request-scoped language negotiation. Never use process-global locale mutation.
use aow_i18n::Language;
use axum::{
    extract::Request,
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};

fn negotiate(value: &str) -> Language {
    let mut selected = Language::English;
    let mut highest = 0.0_f32;
    for entry in value.split(',') {
        let mut parts = entry.trim().split(';');
        let tag = parts.next().unwrap_or_default().trim();
        let mut quality = 1.0_f32;
        for part in parts {
            if let Some(value) = part.trim().strip_prefix("q=") {
                quality = value.parse::<f32>().unwrap_or(0.0);
            }
        }
        if !quality.is_finite() || quality <= 0.0 || quality > 1.0 {
            quality = 0.0;
        }
        let normalized = tag.to_ascii_lowercase().replace('_', "-");
        if (normalized.starts_with("en-")
            || matches!(
                normalized.as_str(),
                "en" | "en-us" | "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" | "*"
            ))
            && quality > highest
        {
            selected = Language::parse(tag);
            highest = quality;
        }
    }
    selected
}

pub(crate) async fn middleware(request: Request, next: Next) -> Response {
    let header_language = request
        .headers()
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|value| value.to_str().ok())
        .map(negotiate)
        .unwrap_or_default();
    let language = if request
        .headers()
        .get(header::UPGRADE)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"))
    {
        axum::extract::Query::<std::collections::HashMap<String, String>>::try_from_uri(
            request.uri(),
        )
        .ok()
        .and_then(|query| query.get("lang").map(|value| Language::parse(value)))
        .unwrap_or(header_language)
    } else {
        header_language
    };
    let mut response = aow_i18n::scope(language, next.run(request)).await;
    response.headers_mut().insert(
        header::CONTENT_LANGUAGE,
        HeaderValue::from_static(language.tag()),
    );
    response
        .headers_mut()
        .append(header::VARY, HeaderValue::from_static("Accept-Language"));
    response
}

/// Axum spawns upgrade callbacks internally, outside request task-local scope.
/// Capture the negotiated locale before handing the callback to Axum.
pub(crate) fn upgrade<F, Fut>(websocket: axum::extract::WebSocketUpgrade, callback: F) -> Response
where
    F: FnOnce(axum::extract::ws::WebSocket) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let language = aow_i18n::current_language();
    websocket
        .on_upgrade(move |socket| aow_i18n::scope(language, async move { callback(socket).await }))
}

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
