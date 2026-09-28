use super::*;

pub(super) async fn upload_terminal_clipboard_image(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, HttpError> {
    super::origin::validate_request_origin(&headers).map_err(terminal_http_error)?;
    if let Some(content_length) = headers.get(CONTENT_LENGTH) {
        let content_length = content_length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                terminal_http_error(TerminalError::Invalid(
                    "Content-Length must be a non-negative integer".to_owned(),
                ))
            })?;
        if content_length > MAX_CLIPBOARD_IMAGE_BYTES {
            return Err(terminal_http_error(TerminalError::ClipboardImageTooLarge));
        }
    }
    let image = state
        .terminals
        .store_clipboard_image(&tab_id, &pane_id, body)
        .await
        .map_err(terminal_http_error)?;
    Ok((StatusCode::CREATED, Json(image)))
}
