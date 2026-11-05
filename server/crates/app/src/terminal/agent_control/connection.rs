use super::super::*;
use aow_runtime_contract::terminal::{
    TerminalAttachClientMessage, TerminalAttachServerMessage, TerminalControlState,
};
use tokio_tungstenite::tungstenite;

pub(in crate::terminal) struct AgentConnection {
    pub(in crate::terminal) socket: RuntimeSocket,
}

impl AgentConnection {
    #[cfg(test)]
    pub(in crate::terminal) async fn claim(
        client: &TerminalClient,
        pane_id: &str,
    ) -> Result<Self, TerminalError> {
        Self::claim_with_force(client, pane_id, false).await
    }

    pub(in crate::terminal) async fn claim_with_force(
        client: &TerminalClient,
        pane_id: &str,
        force: bool,
    ) -> Result<Self, TerminalError> {
        let socket = client
            .attach_controlled_with_resume(pane_id, None, None)
            .await
            .map_err(map_client_error)?;
        let mut connection = Self { socket };
        connection
            .send(TerminalAttachClientMessage::Claim { force })
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

    pub(in crate::terminal) fn check(
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

    pub(in crate::terminal) async fn send(
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

    #[cfg(test)]
    pub(in crate::terminal) async fn write(&mut self, data: String) -> Result<(), TerminalError> {
        let request_id = aow_id::new_id();
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

    pub(in crate::terminal) async fn submit(&mut self, task: &str) -> Result<(), TerminalError> {
        TerminalClient::submit_agent_on_socket(&mut self.socket, task)
            .await
            .map_err(map_client_error)
    }
}
