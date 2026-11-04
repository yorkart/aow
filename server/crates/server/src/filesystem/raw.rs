use super::paths::decode_absolute;
use crate::{AppState, HttpError};
use axum::{
    extract::{Path as AxumPath, State},
    http::HeaderMap,
    response::Response,
};
use std::path::PathBuf;
#[cfg(test)]
use {
    super::paths::PATH_SEGMENT_ENCODE_SET,
    aow_filesystem::open_file,
    axum::{
        body::Body,
        http::{
            HeaderValue, StatusCode,
            header::{
                ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
                CONTENT_TYPE, RANGE,
            },
        },
    },
    percent_encoding::utf8_percent_encode,
    std::io::SeekFrom,
    tokio::io::{AsyncReadExt, AsyncSeekExt},
    tokio_util::io::ReaderStream,
};
pub(crate) async fn raw_file(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let path = decode_absolute(&path)?;
    raw_response(&state, path, headers, false).await
}

pub(super) async fn raw_response(
    state: &AppState,
    path: PathBuf,
    headers: HeaderMap,
    attachment: bool,
) -> Result<Response, HttpError> {
    if state.acp.runtime().is_some() {
        return super::node::download(state, &path, &headers, attachment).await;
    }
    #[cfg(not(test))]
    {
        let _ = (path, headers, attachment);
        Err(super::node::runtime(state).unwrap_err())
    }
    #[cfg(test)]
    {
        let (mut file, metadata) = open_file(&path).await?;
        let size = metadata.len();
        let content_type = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string();
        let range = headers.get(RANGE).and_then(|value| value.to_str().ok());
        let (start, end, status) = match range {
            Some(value) => {
                let (start, end) = parse_range(value, size)?;
                (start, end, StatusCode::PARTIAL_CONTENT)
            }
            None => (0, size.saturating_sub(1), StatusCode::OK),
        };
        let length = if size == 0 { 0 } else { end - start + 1 };
        file.seek(SeekFrom::Start(start)).await?;
        let stream = ReaderStream::new(file.take(length));
        let body = Body::from_stream(stream);
        let mut response = Response::new(body);
        *response.status_mut() = status;
        let response_headers = response.headers_mut();
        response_headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        response_headers.insert(CONTENT_TYPE, HeaderValue::from_str(&content_type).unwrap());
        // Files are untrusted content, even when the reader is authenticated.
        // Restrict inline rendering and sandbox documents opened directly as well
        // as in an iframe; HttpOnly alone cannot prevent same-origin API calls.
        response_headers.insert(
            "x-content-type-options",
            HeaderValue::from_static("nosniff"),
        );
        response_headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        let safe_inline = matches!(
            content_type.as_str(),
            "text/plain"
                | "application/pdf"
                | "image/png"
                | "image/jpeg"
                | "image/gif"
                | "image/webp"
                | "image/avif"
                | "image/bmp"
                | "image/x-icon"
                | "image/vnd.microsoft.icon"
                | "audio/mpeg"
                | "audio/ogg"
                | "audio/wav"
                | "audio/mp4"
                | "video/mp4"
                | "video/webm"
                | "video/ogg"
        );
        // PDF viewers are browser plugins and cannot run in a sandboxed document.
        // Only allow known inert media types inline; sandbox all other file types.
        if !safe_inline {
            response_headers.insert(
                "content-security-policy",
                HeaderValue::from_static("sandbox; default-src 'none'"),
            );
        }
        if attachment || !safe_inline {
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            response_headers.insert(
                CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!(
                    "attachment; filename*=UTF-8''{}",
                    utf8_percent_encode(&filename, PATH_SEGMENT_ENCODE_SET)
                ))
                .unwrap(),
            );
        }
        response_headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string()).unwrap(),
        );
        if status == StatusCode::PARTIAL_CONTENT {
            response_headers.insert(
                CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {start}-{end}/{size}")).unwrap(),
            );
        }
        Ok(response)
    }
}

#[cfg(test)]
fn parse_range(value: &str, size: u64) -> Result<(u64, u64), HttpError> {
    if size == 0 || !value.starts_with("bytes=") || value.contains(',') {
        return Err(HttpError::range_not_satisfiable(size));
    }
    let (start, end) = value[6..]
        .split_once('-')
        .ok_or_else(|| HttpError::range_not_satisfiable(size))?;
    let (start, end) = if start.is_empty() {
        let suffix = end
            .parse::<u64>()
            .map_err(|_| HttpError::range_not_satisfiable(size))?;
        let suffix = suffix.min(size);
        (size - suffix, size - 1)
    } else {
        let start = start
            .parse::<u64>()
            .map_err(|_| HttpError::range_not_satisfiable(size))?;
        let end = if end.is_empty() {
            size - 1
        } else {
            end.parse::<u64>()
                .map_err(|_| HttpError::range_not_satisfiable(size))?
                .min(size - 1)
        };
        (start, end)
    };
    if start >= size || start > end {
        return Err(HttpError::range_not_satisfiable(size));
    }
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(parse_range("bytes=2-5", 10).unwrap(), (2, 5));
        assert_eq!(parse_range("bytes=7-", 10).unwrap(), (7, 9));
        assert_eq!(parse_range("bytes=-3", 10).unwrap(), (7, 9));
        assert!(parse_range("bytes=10-11", 10).is_err());
    }
}
