use axum::body::Body;
use chrono::{SecondsFormat, Utc};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use super::super::*;
use super::{clipboard_image_type, storage::open_private_file};

impl TerminalManager {
    pub(in crate::terminal) async fn store_clipboard_image(
        &self,
        tab_id: &str,
        pane_id: &str,
        body: Body,
    ) -> Result<ClipboardImageResponse, TerminalError> {
        self.ensure_pane(tab_id, pane_id)?;
        let _operation = self.inner.clipboard_operation.lock().await;
        let used_bytes = self.inner.clipboard.clean_expired()?;
        let upload_id = aow_id::new_id();
        let temp_path = self
            .inner
            .clipboard
            .directory
            .join(format!(".{}.tmp", upload_id));
        let result = self
            .write_clipboard_image(tab_id, pane_id, body, upload_id, &temp_path, used_bytes)
            .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temp_path).await;
        }
        result
    }

    async fn write_clipboard_image(
        &self,
        tab_id: &str,
        pane_id: &str,
        body: Body,
        upload_id: String,
        temp_path: &Path,
        used_bytes: u64,
    ) -> Result<ClipboardImageResponse, TerminalError> {
        let mut file = open_private_file(temp_path).await?;
        let mut stream = body.into_data_stream();
        let mut size = 0_u64;
        let mut image_bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                TerminalError::Invalid(format!("invalid request body: {error}"))
            })?;
            size = size
                .checked_add(chunk.len() as u64)
                .ok_or(TerminalError::ClipboardImageTooLarge)?;
            if size > MAX_CLIPBOARD_IMAGE_BYTES {
                return Err(TerminalError::ClipboardImageTooLarge);
            }
            if used_bytes.saturating_add(size) > MAX_CLIPBOARD_DIRECTORY_BYTES {
                return Err(TerminalError::ClipboardQuotaExceeded);
            }
            image_bytes.extend_from_slice(&chunk);
            file.write_all(&chunk).await?;
        }
        let image_type =
            clipboard_image_type(&image_bytes).ok_or(TerminalError::UnsupportedClipboardImage)?;
        file.sync_all().await?;
        drop(file);
        self.ensure_pane(tab_id, pane_id)?;

        let final_path = self
            .inner
            .clipboard
            .directory
            .join(format!("{}.{}", upload_id, image_type.extension));
        {
            let state = self.lock_state()?;
            let tab = state
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .ok_or_else(|| TerminalError::TabNotFound(tab_id.to_owned()))?;
            if !tab.panes.iter().any(|pane| pane.id == pane_id) {
                return Err(TerminalError::PaneNotFound(pane_id.to_owned()));
            }
            std::fs::rename(temp_path, &final_path)?;
        }
        let expires_at = (Utc::now() + chrono::Duration::from_std(CLIPBOARD_IMAGE_TTL).unwrap())
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        Ok(ClipboardImageResponse {
            path: final_path.to_string_lossy().into_owned(),
            mime: image_type.mime,
            size,
            expires_at,
        })
    }
}
