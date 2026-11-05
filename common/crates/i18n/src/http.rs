//! Request-scoped language negotiation. Never use process-global locale mutation.
use crate::Language;
use axum::{
    extract::Request,
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};

pub fn negotiate(value: &str) -> Language {
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

pub async fn middleware(request: Request, next: Next) -> Response {
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
    let mut response = crate::scope(language, next.run(request)).await;
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
pub fn upgrade<F, Fut>(websocket: axum::extract::WebSocketUpgrade, callback: F) -> Response
where
    F: FnOnce(axum::extract::ws::WebSocket) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let language = crate::current_language();
    websocket
        .on_upgrade(move |socket| crate::scope(language, async move { callback(socket).await }))
}
