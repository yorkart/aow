use super::super::*;
use axum::body::Body;
impl TerminalManager {
    pub(in crate::terminal) async fn store_clipboard_image(
        &self,
        tab_id: &str,
        pane_id: &str,
        body: Body,
    ) -> Result<ClipboardImageResponse, TerminalError> {
        self.ensure_pane(tab_id, pane_id)?;
        let response = self
            .inner
            .terminald
            .host()
            .workspace_clipboard(reqwest::Body::wrap_stream(body.into_data_stream()))
            .await
            .map_err(|error| match error {
                HostError::HttpStatus {
                    status: StatusCode::PAYLOAD_TOO_LARGE,
                    ..
                } => TerminalError::ClipboardImageTooLarge,
                HostError::HttpStatus {
                    status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    ..
                } => TerminalError::UnsupportedClipboardImage,
                HostError::HttpStatus {
                    status: StatusCode::INSUFFICIENT_STORAGE,
                    ..
                } => TerminalError::ClipboardQuotaExceeded,
                error => map_client_error(error),
            })?;
        self.ensure_pane(tab_id, pane_id)?;
        Ok(response)
    }
}
