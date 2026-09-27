use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode, Uri, header::LOCATION},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// A deployment prefix, stored without a trailing slash (empty for root).
/// Restrict it to unambiguous URL segments so HTML, cookies and proxy routing
/// all use exactly the same spelling. Filesystem paths are never passed here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BasePath(String);

impl BasePath {
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        if value.is_empty() || value == "/" {
            return Ok(Self::default());
        }
        let path = value.strip_suffix('/').unwrap_or(value);
        anyhow::ensure!(
            path.len() <= 1024
                && path.starts_with('/')
                && path[1..].split('/').all(|segment| {
                    !segment.is_empty()
                        && !matches!(segment, "." | "..")
                        && segment
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
                }),
            "invalid base path: use / or slash-separated URL segments containing only letters, digits, -, ., _, ~ (no . or .. segments)"
        );
        Ok(Self(path.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.0)
    }

    pub(crate) fn cookie_name(&self) -> String {
        if self.0.is_empty() {
            "aow_session".to_owned()
        } else {
            format!("aow_session_{:x}", md5::compute(self.0.as_bytes()))
        }
    }

    pub(crate) fn inject_html(&self, html: &str) -> String {
        // Vite builds relative assets. The base tag also resolves assets when
        // index.html is served for a deeply nested client-side route.
        html.replacen(
            "<head>",
            &format!(
                "<head><base href=\"{}/\"><meta name=\"aow-base-path\" content=\"{}\">",
                self.0, self.0
            ),
            1,
        )
    }
}

/// Strip the configured prefix before the inner router matches a route. This
/// keeps authentication (including the exact public-share allowlist) operating
/// on application paths, and leaves WebSocket/SSE bodies completely untouched.
pub(crate) async fn mount(
    State(base): State<BasePath>,
    mut request: Request,
    next: Next,
) -> Response {
    if !base.0.is_empty() {
        let path = request.uri().path();
        if path == base.0 {
            let query = request
                .uri()
                .query()
                .map(|q| format!("?{q}"))
                .unwrap_or_default();
            return (
                StatusCode::PERMANENT_REDIRECT,
                [(LOCATION, format!("{}/{query}", base.0))],
            )
                .into_response();
        }
        let Some(stripped) = path.strip_prefix(&base.0).filter(|p| p.starts_with('/')) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let mut parts = request.uri().clone().into_parts();
        let query = request
            .uri()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
        let Ok(path_and_query) = format!("{stripped}{query}").parse() else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        parts.path_and_query = Some(path_and_query);
        *request.uri_mut() = Uri::from_parts(parts).expect("valid URI with updated path");
    }
    let mut response = next.run(request).await;
    if let Some(location) = response
        .headers()
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        && location.starts_with('/')
        && !location.starts_with("//")
        && let Ok(location) = HeaderValue::from_str(&base.url(location))
    {
        response.headers_mut().insert(LOCATION, location);
    }
    response
}

#[cfg(test)]
mod tests;
