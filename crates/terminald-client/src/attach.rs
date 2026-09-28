use tokio::net::UnixStream;
use tokio_tungstenite::{WebSocketStream, client_async};

use super::{TerminaldClient, TerminaldClientError, paths::attach_uri};

pub type TerminaldAttachStream = WebSocketStream<UnixStream>;

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
        let stream = UnixStream::connect(&self.socket).await?;
        let (socket, _) = client_async(
            attach_uri(id, epoch, after, controlled, vt_snapshot, observer),
            stream,
        )
        .await?;
        Ok(socket)
    }

    pub async fn connect_attach(
        &self,
        id: &str,
    ) -> Result<TerminaldAttachStream, TerminaldClientError> {
        self.attach(id).await
    }
}
