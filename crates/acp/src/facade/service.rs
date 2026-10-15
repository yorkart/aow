//! Stable service surface. Upstream-dependent behavior belongs in agent_servers/acp_thread.
use super::{AcpHost, AgentInfo, ConnectionInfo, Content, SessionInfo, SessionSnapshot};
use crate::agent_servers::AgentServerStore;
use anyhow::Result;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct AcpService(AgentServerStore);
impl AcpService {
    pub fn subscribe_sync(&self) -> tokio::sync::broadcast::Receiver<super::SyncEvent> {
        self.0.subscribe_sync()
    }
    pub fn sync_event(&self, resource: &super::SyncResource) -> super::SyncEvent {
        self.0.sync_event(resource)
    }
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
    pub fn connection_requests(&self, connection_id: &str) -> Result<Vec<super::Permission>> {
        self.0.connection_requests(connection_id)
    }
    pub fn answer_connection(
        &self,
        connection_id: &str,
        request_id: &str,
        response: Value,
    ) -> Result<()> {
        self.0
            .answer_connection(connection_id, request_id, response)
    }
    pub fn disconnect(&self, connection_id: &str) -> Result<()> {
        self.0.disconnect(connection_id)
    }
    pub async fn authenticate(&self, connection_id: &str, method_id: &str) -> Result<()> {
        self.0.authenticate(connection_id, method_id).await
    }
    pub async fn remote_sessions(
        &self,
        connection_id: &str,
        cursor: Option<String>,
    ) -> Result<Value> {
        self.0.remote_sessions(connection_id, cursor).await
    }
    pub fn import_sessions(
        &self,
        connection_id: &str,
        sessions: Vec<super::SessionImport>,
    ) -> Result<Vec<SessionInfo>> {
        self.0.import_sessions(connection_id, sessions)
    }
    pub async fn new_session(&self, connection_id: &str) -> Result<SessionSnapshot> {
        self.0.new_session(connection_id).await
    }
    /// Persist identity before connecting; the controlling view then calls resume.
    pub async fn prepare_session(&self, agent_id: &str, cwd: PathBuf) -> Result<SessionSnapshot> {
        self.0.prepare_session(agent_id, cwd).await
    }
    /// Resolve links from metadata without hydrating the transcript or starting an adapter.
    pub fn session_info(&self, thread_id: &str) -> Result<SessionInfo> {
        self.0.session_info(thread_id)
    }
    pub fn session_connection_info(&self, thread_id: &str) -> Result<Option<ConnectionInfo>> {
        self.0.session_connection_info(thread_id)
    }
    pub async fn load_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<SessionSnapshot> {
        self.0.load_session(connection_id, session_id).await
    }
    pub async fn resume(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.0.resume(thread_id).await
    }
    /// Finish queued SQLite writes before shutting down the host runtime.
    pub fn flush_threads_on_quit(&self) -> Result<()> {
        self.0.flush_threads_on_quit()
    }
    pub fn sessions(&self, cwd: Option<&str>) -> Vec<SessionInfo> {
        self.0.sessions(cwd)
    }
    pub fn snapshot(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.0.snapshot(thread_id)
    }
    pub fn overview(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.0.overview(thread_id)
    }
    pub fn overview_updates(
        &self,
        thread_id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<super::SessionUpdates> {
        self.0.overview_updates(thread_id, generation, revision)
    }
    pub fn process(
        &self,
        thread_id: &str,
        turn_id: &str,
        generation: Option<&str>,
        revision: Option<u64>,
    ) -> Result<super::SessionUpdates> {
        self.0.process(thread_id, turn_id, generation, revision)
    }
    pub fn updates(
        &self,
        thread_id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<super::SessionUpdates> {
        self.0.updates(thread_id, generation, revision)
    }
    pub fn logs(&self, connection_id: &str) -> Result<Vec<Value>> {
        self.0.logs(connection_id)
    }
    pub fn session_logs(&self, thread_id: &str) -> Result<Vec<Value>> {
        self.0.session_logs(thread_id)
    }
    pub fn start_prompt(&self, thread_id: &str, content: Vec<Content>) -> Result<()> {
        self.0.start_prompt(thread_id, content)
    }
    pub fn cancel(&self, thread_id: &str) -> Result<()> {
        self.0.cancel(thread_id)
    }
    pub fn dismiss_notice(&self, thread_id: &str, notice_id: &str) -> Result<()> {
        self.0.dismiss_notice(thread_id, notice_id)
    }
    pub fn clear_plan(&self, thread_id: &str) -> Result<()> {
        self.0.clear_plan(thread_id)
    }
    pub async fn set_mode(&self, thread_id: &str, mode: &str) -> Result<()> {
        self.0.set_mode(thread_id, mode).await
    }
    pub async fn set_config_option(
        &self,
        thread_id: &str,
        option: &str,
        value: Value,
    ) -> Result<()> {
        self.0.set_config_option(thread_id, option, value).await
    }
    pub fn answer(&self, thread_id: &str, request: &str, response: Value) -> Result<()> {
        self.0.answer(thread_id, request, response)
    }
    pub async fn close_session(&self, thread_id: &str) -> Result<()> {
        self.0.close_session(thread_id).await
    }
    pub async fn delete_session(&self, thread_id: &str) -> Result<()> {
        self.0.delete_session(thread_id).await
    }
}
