//! Agent capabilities and operations exposed by the execution node.
use crate::{RuntimeClient, RuntimeError, TerminalClient};
pub use aow_runtime_contract::agents::*;
impl RuntimeClient {
    pub async fn agent_role_layout(
        &self,
        request: &ResolveRoleLayout,
    ) -> Result<String, RuntimeError> {
        self.post_json("/v1/agents/role-layout", request).await
    }

    pub async fn probe_agent(&self, request: &ProbeAgent) -> Result<(), RuntimeError> {
        self.post_json("/v1/agents/probe", request).await
    }

    pub async fn agent_configuration_environment(
        &self,
    ) -> Result<std::collections::BTreeMap<String, String>, RuntimeError> {
        self.get_json("/v1/agents/configuration-environment").await
    }

    pub async fn agent_capabilities(&self) -> Result<Vec<AgentCapabilities>, RuntimeError> {
        self.get_json("/v1/agents/capabilities").await
    }
    pub async fn discover_agents(
        &self,
        request: &DiscoverAgents,
    ) -> Result<Vec<AgentRegistration>, RuntimeError> {
        self.post_json("/v1/agents/discover", request).await
    }
    pub async fn resolve_agent(&self, request: &ResolveAgent) -> Result<AgentLaunch, RuntimeError> {
        self.post_json("/v1/agents/resolve", request).await
    }
    pub async fn prepare_agent_terminal(
        &self,
        request: &PrepareTerminal,
    ) -> Result<AgentLaunch, RuntimeError> {
        self.post_json("/v1/agents/prepare-terminal", request).await
    }
    pub async fn prepare_agent_role(
        &self,
        request: &PrepareRole,
    ) -> Result<AgentLaunch, RuntimeError> {
        self.post_json("/v1/agents/prepare-role", request).await
    }
    pub async fn prepare_role_session(
        &self,
        request: &PrepareRoleSession,
    ) -> Result<RoleSessionPlan, RuntimeError> {
        self.post_json("/v1/agents/prepare-role-session", request)
            .await
    }
    pub async fn validate_agent_automation(
        &self,
        request: &ValidateAutomation,
    ) -> Result<(), RuntimeError> {
        self.post_json("/v1/agents/validate-automation", request)
            .await
    }
    pub async fn match_terminal_profiles(
        &self,
        request: &MatchTerminalProfile,
    ) -> Result<Vec<String>, RuntimeError> {
        self.post_json("/v1/agents/match-terminal", request).await
    }
    pub async fn discovered_execution_path(&self) -> Result<Vec<std::path::PathBuf>, RuntimeError> {
        self.get_json("/v1/environment/discovered-path").await
    }
    pub async fn execution_path(&self) -> Result<Vec<std::path::PathBuf>, RuntimeError> {
        #[derive(serde::Deserialize)]
        struct Environment {
            execution_path: Vec<std::path::PathBuf>,
        }
        Ok(self
            .get_json::<Environment>("/v1/environment")
            .await?
            .execution_path)
    }
    pub async fn set_execution_path(
        &self,
        paths: &[std::path::PathBuf],
    ) -> Result<(), RuntimeError> {
        let _: serde_json::Value = self
            .put_json(
                "/v1/environment",
                &serde_json::json!({"execution_path":paths}),
            )
            .await?;
        Ok(())
    }
}
impl TerminalClient {
    pub async fn submit_agent_on_socket(
        socket: &mut crate::RuntimeSocket,
        task: &str,
    ) -> Result<(), RuntimeError> {
        use aow_protocol::{
            TerminalAttachClientMessage, TerminalAttachServerMessage, TerminalControlState,
            TerminalPaneStatus,
        };
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let request_id = uuid::Uuid::new_v4().to_string();
        socket
            .send(Message::Text(
                serde_json::to_string(&TerminalAttachClientMessage::Submit {
                    request_id: request_id.clone(),
                    task: task.into(),
                })?
                .into(),
            ))
            .await?;
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<TerminalAttachServerMessage>(&text)? {
                        TerminalAttachServerMessage::Written {
                            request_id: received,
                        } if received == request_id => return Ok(()),
                        TerminalAttachServerMessage::Error { message, .. } => {
                            return Err(RuntimeError::Io(std::io::Error::other(message)));
                        }
                        TerminalAttachServerMessage::Control { state }
                            if state != TerminalControlState::Claimed =>
                        {
                            return Err(RuntimeError::Io(std::io::Error::other(
                                "Terminal control was lost",
                            )));
                        }
                        TerminalAttachServerMessage::Status { status, .. }
                            if status != TerminalPaneStatus::Running =>
                        {
                            return Err(RuntimeError::Io(std::io::Error::other("Agent exited")));
                        }
                        _ => {}
                    }
                }
                None | Some(Ok(Message::Close(_))) => {
                    return Err(RuntimeError::Io(std::io::Error::other(
                        "Terminal connection closed",
                    )));
                }
                Some(Err(error)) => return Err(error.into()),
                _ => {}
            }
        }
    }

    /// Retries transport failures using the same receipt ID. Execution is owned by the node.
    pub async fn run_agent_operation(
        &self,
        id: &str,
        request: &TerminalAgentRequest,
    ) -> Result<TerminalAgentProgress, RuntimeError> {
        use std::time::Duration;
        let seconds = match request.operation {
            TerminalAgentOperation::Initialize {
                timeout_seconds, ..
            } => timeout_seconds.min(600),
            TerminalAgentOperation::Submit { .. } => 15,
        };
        tokio::time::timeout(Duration::from_secs(seconds + 15), async {
            let mut admitted = false;
            loop {
                let result = if admitted {
                    self.agent_operation_status(id, &request.request_id).await
                } else {
                    self.agent_operation(id, request).await
                };
                match result {
                    Ok(progress) => {
                        admitted = true;
                        if progress.finished {
                            return Ok(progress);
                        }
                    }
                    Err(error) if retryable(&error) => {}
                    Err(error) => return Err(error),
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        })
        .await
        .map_err(|_| {
            RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Agent operation status unavailable; query the same operation ID before retrying",
            ))
        })?
    }

    pub async fn agent_operation(
        &self,
        id: &str,
        request: &TerminalAgentRequest,
    ) -> Result<TerminalAgentProgress, RuntimeError> {
        self.runtime()
            .post_json(
                &format!("/v1/terminal/runtimes/{}/agent-operation", path_segment(id)),
                request,
            )
            .await
    }
    pub async fn agent_operation_status(
        &self,
        id: &str,
        request_id: &str,
    ) -> Result<TerminalAgentProgress, RuntimeError> {
        self.runtime()
            .get_json(&format!(
                "/v1/terminal/runtimes/{}/agent-operation/{}",
                path_segment(id),
                path_segment(request_id)
            ))
            .await
    }
}

fn path_segment(id: &str) -> String {
    percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn retryable(error: &RuntimeError) -> bool {
    match error {
        RuntimeError::Http(_) | RuntimeError::Io(_) => true,
        RuntimeError::HttpStatus { status, .. } => status.is_server_error(),
        _ => false,
    }
}
