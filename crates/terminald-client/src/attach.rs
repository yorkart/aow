use tokio::net::UnixStream;
use tokio_tungstenite::{
    WebSocketStream, client_async_with_config, tungstenite::protocol::WebSocketConfig,
};

use super::{TerminaldClient, TerminaldClientError, paths::attach_uri};

pub type TerminaldAttachStream = WebSocketStream<UnixStream>;

#[derive(Clone, Copy, Default)]
pub struct TerminalAttachOptions<'a> {
    pub epoch: Option<&'a str>,
    pub after: Option<u64>,
    pub controlled: bool,
    pub vt_snapshot: bool,
    pub observer: bool,
    pub gzip_snapshot: bool,
}

impl TerminaldClient {
    pub async fn attach(&self, id: &str) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume(id, None, None).await
    }

    pub async fn attach_from(
        &self,
        id: &str,
        epoch: &str,
        after: u64,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume(id, Some(epoch), Some(after)).await
    }

    pub async fn attach_with_resume(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_capabilities(id, epoch, after, false)
            .await
    }

    /// Connect using legacy auto-claim semantics while explicitly negotiating
    /// geometry-aware VT snapshot restores. This is primarily useful for
    /// direct terminald clients and end-to-end compatibility tests.
    pub async fn attach_with_resume_capabilities(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        vt_snapshot: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_mode(id, epoch, after, false, vt_snapshot, false)
            .await
    }

    /// Connect using the opt-in controller protocol. Unlike the legacy attach
    /// endpoint behavior, this connection stays neutral until it sends a
    /// `claim` control message.
    pub async fn attach_controlled_with_resume(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_controlled_with_resume_capabilities(id, epoch, after, false)
            .await
    }

    /// Connect using controller v2 and explicitly negotiate support for the
    /// geometry-aware VT snapshot restore extension. Existing callers remain
    /// on raw replay unless they opt in here.
    pub async fn attach_controlled_with_resume_capabilities(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        vt_snapshot: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_mode(id, epoch, after, true, vt_snapshot, false)
            .await
    }

    /// Connect using controller v2 and opt into read-only output subscriptions
    /// when another attachment already owns terminal input.
    pub async fn attach_controlled_with_resume_capabilities_and_observer(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        vt_snapshot: bool,
        observer: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_resume_mode(id, epoch, after, true, vt_snapshot, observer)
            .await
    }

    async fn attach_with_resume_mode(
        &self,
        id: &str,
        epoch: Option<&str>,
        after: Option<u64>,
        controlled: bool,
        vt_snapshot: bool,
        observer: bool,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach_with_options(
            id,
            TerminalAttachOptions {
                epoch,
                after,
                controlled,
                vt_snapshot,
                observer,
                gzip_snapshot: false,
            },
        )
        .await
    }

    pub async fn attach_with_options(
        &self,
        id: &str,
        options: TerminalAttachOptions<'_>,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        let stream = UnixStream::connect(&self.socket).await?;
        // An 8 MiB ANSI snapshot can expand up to 6x inside JSON. The default
        // 16 MiB frame limit rejects valid restores before the Web proxy sees
        // them. Keep aligned with terminald's maximum socket write buffer.
        let config = WebSocketConfig::default().max_frame_size(Some(64 * 1024 * 1024));
        let mut uri = attach_uri(
            id,
            options.epoch,
            options.after,
            options.controlled,
            options.vt_snapshot,
            options.observer,
        );
        if options.vt_snapshot && options.gzip_snapshot {
            uri.push_str("&snapshot_compression=gzip");
        }
        let (socket, _) = client_async_with_config(uri, stream, Some(config)).await?;
        Ok(socket)
    }

    pub async fn connect_attach(
        &self,
        id: &str,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach(id).await
    }
}
