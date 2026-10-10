//! Stable service surface. Upstream-dependent behavior belongs in agent_servers/acp_thread.
use super::{AcpHost, AgentInfo, ConnectionInfo, Content, SessionInfo, SessionSnapshot};
use crate::agent_servers::AgentServerStore;
use anyhow::Result;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct AcpService(AgentServerStore);
impl AcpService {
    pub fn new(host: Arc<dyn AcpHost>, state_directory: Option<PathBuf>) -> Result<Self> {
        Ok(Self(AgentServerStore::new(host, state_directory)?))
    }
    pub async fn agents(&self) -> Result<Vec<AgentInfo>> {
        self.0.agents().await
    }
    pub async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        self.0.registry(refresh).await
    }
    pub async fn install_agent(&self, agent_id: &str) -> Result<()> {
        self.0.install_agent(agent_id).await
    }
    pub fn installation_status(&self, agent_id: &str) -> Option<super::ConnectionStatus> {
        self.0.installation_status(agent_id)
    }
    pub async fn connect(&self, agent_id: &str, cwd: PathBuf) -> Result<ConnectionInfo> {
        self.0.connect(agent_id, cwd).await
    }
    pub async fn connection_status(
        &self,
        agent_id: &str,
        cwd: PathBuf,
    ) -> Result<Option<super::ConnectionStatus>> {
        self.0.connection_status(agent_id, cwd).await
    }
    pub fn connection_requests(&self, id: &str) -> Result<Vec<super::Permission>> {
        self.0.connection_requests(id)
    }
    pub fn answer_connection(&self, id: &str, request_id: &str, response: Value) -> Result<()> {
        self.0.answer_connection(id, request_id, response)
    }
    pub fn disconnect(&self, id: &str) -> Result<()> {
        self.0.disconnect(id)
    }
    pub async fn authenticate(&self, id: &str, method_id: &str) -> Result<()> {
        self.0.authenticate(id, method_id).await
    }
    pub async fn remote_sessions(&self, id: &str, cursor: Option<String>) -> Result<Value> {
        self.0.remote_sessions(id, cursor).await
    }
    pub fn import_sessions(
        &self,
        id: &str,
        sessions: Vec<super::SessionImport>,
    ) -> Result<Vec<SessionInfo>> {
        self.0.import_sessions(id, sessions)
    }
    pub async fn new_session(&self, id: &str) -> Result<SessionSnapshot> {
        self.0.new_session(id).await
    }
    pub async fn load_session(&self, id: &str, remote_id: &str) -> Result<SessionSnapshot> {
        self.0.load_session(id, remote_id).await
    }
    pub async fn resume(&self, id: &str) -> Result<SessionSnapshot> {
        self.0.resume(id).await
    }
    pub fn sessions(&self, cwd: Option<&str>) -> Vec<SessionInfo> {
        self.0.sessions(cwd)
    }
    pub fn snapshot(&self, id: &str) -> Result<SessionSnapshot> {
        self.0.snapshot(id)
    }
    pub fn logs(&self, id: &str) -> Result<Vec<Value>> {
        self.0.logs(id)
    }
    pub fn session_logs(&self, id: &str) -> Result<Vec<Value>> {
        self.0.session_logs(id)
    }
    pub fn start_prompt(&self, id: &str, content: Vec<Content>) -> Result<()> {
        self.0.start_prompt(id, content)
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        self.0.cancel(id)
    }
    pub fn dismiss_notice(&self, id: &str, notice_id: &str) -> Result<()> {
        self.0.dismiss_notice(id, notice_id)
    }
    pub async fn set_mode(&self, id: &str, mode: &str) -> Result<()> {
        self.0.set_mode(id, mode).await
    }
    pub async fn set_config_option(&self, id: &str, option: &str, value: Value) -> Result<()> {
        self.0.set_config_option(id, option, value).await
    }
    pub fn answer(&self, id: &str, request: &str, response: Value) -> Result<()> {
        self.0.answer(id, request, response)
    }
    pub async fn close_session(&self, id: &str) -> Result<()> {
        self.0.close_session(id).await
    }
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        self.0.delete_session(id).await
    }
}
