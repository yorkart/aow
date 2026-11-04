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
        #[cfg(test)]
        if !self
            .inner
            .node_workspace
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return self
                .store_local_clipboard_image(tab_id, pane_id, body)
                .await;
        }
        let response = self
            .inner
            .terminald
            .runtime()
            .workspace_clipboard(reqwest::Body::wrap_stream(body.into_data_stream()))
            .await
            .map_err(|error| match error {
                RuntimeError::HttpStatus {
                    status: StatusCode::PAYLOAD_TOO_LARGE,
                    ..
                } => TerminalError::ClipboardImageTooLarge,
                RuntimeError::HttpStatus {
                    status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    ..
                } => TerminalError::UnsupportedClipboardImage,
                RuntimeError::HttpStatus {
                    status: StatusCode::INSUFFICIENT_STORAGE,
                    ..
                } => TerminalError::ClipboardQuotaExceeded,
                error => map_client_error(error),
            })?;
        self.ensure_pane(tab_id, pane_id)?;
        serde_json::from_value(response).map_err(TerminalError::Json)
    }
}
