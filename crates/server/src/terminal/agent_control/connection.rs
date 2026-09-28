use super::super::*;
use aow_protocol::{
    TerminalAttachClientMessage, TerminalAttachServerMessage, TerminalControlState,
};
use tokio_tungstenite::tungstenite;

pub(super) struct AgentConnection {
    pub(super) socket: TerminaldAttachStream,
}

impl AgentConnection {
    pub(super) async fn claim(
        client: &TerminaldClient,
        pane_id: &str,
    ) -> Result<Self, TerminalError> {
        let socket = client
            .attach_controlled_with_resume(pane_id, None, None)
            .await
            .map_err(map_client_error)?;
        let mut connection = Self { socket };
        connection
            .send(TerminalAttachClientMessage::Claim { force: false })
            .await?;
        loop {
            match Self::check(connection.socket.next().await)? {
                Some(TerminalAttachServerMessage::Control {
                    state: TerminalControlState::Claimed,
                }) => return Ok(connection),
                Some(TerminalAttachServerMessage::Control { .. }) => {
                    return Err(TerminalError::Conflict(
                        "pane is controlled by another client".into(),
                    ));
                }
                _ => {}
            }
        }
    }

    pub(super) fn check(
        message: Option<Result<tungstenite::Message, tungstenite::Error>>,
    ) -> Result<Option<TerminalAttachServerMessage>, TerminalError> {
        match message {
            Some(Ok(tungstenite::Message::Text(text))) => {
                let message = serde_json::from_str(&text)?;
                match message {
                    TerminalAttachServerMessage::Error { message, .. } => {
                        Err(TerminalError::Conflict(message))
                    }
                    TerminalAttachServerMessage::Status { status, .. }
                        if status != TerminalPaneStatus::Running =>
                    {
                        Err(TerminalError::Conflict("agent exited".into()))
                    }
                    message => Ok(Some(message)),
                }
            }
            None | Some(Ok(tungstenite::Message::Close(_))) => {
                Err(TerminalError::Conflict("terminal connection closed".into()))
            }
            Some(Err(error)) => Err(TerminalError::Daemon(error.to_string())),
            _ => Ok(None),
        }
    }

    pub(super) async fn send(
        &mut self,
        message: TerminalAttachClientMessage,
    ) -> Result<(), TerminalError> {
        self.socket
            .send(tungstenite::Message::Text(
                serde_json::to_string(&message)?.into(),
            ))
            .await
            .map_err(|error| TerminalError::Daemon(error.to_string()))
    }

    pub(super) async fn write(&mut self, data: String) -> Result<(), TerminalError> {
        let request_id = Uuid::new_v4().to_string();
        self.send(TerminalAttachClientMessage::Write {
            request_id: request_id.clone(),
            data,
        })
        .await?;
        loop {
            if matches!(Self::check(self.socket.next().await)?, Some(TerminalAttachServerMessage::Written { request_id: id }) if id == request_id)
            {
                return Ok(());
            }
        }
    }

    pub(super) async fn submit(&mut self, task: &str) -> Result<(), TerminalError> {
        self.write(format!("\x1b[200~{task}\x1b[201~")).await?;
        // Give the TUI's paste/input event a separate iteration before Enter.
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.write("\r".into()).await
    }

    pub(super) async fn finish(&mut self) -> Result<(), TerminalError> {
        self.socket
            .close(None)
            .await
            .map_err(|error| TerminalError::Daemon(error.to_string()))?;
        // terminald releases the controller before completing the close handshake.
        while let Some(message) = self.socket.next().await {
            match message {
                Ok(tungstenite::Message::Close(_)) => break,
                other => {
                    Self::check(Some(other))?;
                }
            }
        }
        Ok(())
    }
}
