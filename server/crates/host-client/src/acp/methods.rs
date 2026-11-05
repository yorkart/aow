use super::AcpClient;
use anyhow::Result;
use aow_host_contract::acp::*;
use serde_json::Value;
use std::path::PathBuf;
impl AcpClient {
    pub async fn sync_event(&self, resource: &SyncResource) -> Result<SyncEvent> {
        self.0.resources.lock().unwrap().insert(resource.clone());
        self.call(Operation::SyncEvent {
            resource: resource.clone(),
        })
        .await
    }
    pub async fn agents(&self) -> Result<Vec<AgentInfo>> {
        self.call(Operation::Agents).await
    }
    pub async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        self.call(Operation::Registry { refresh }).await
    }
    pub async fn install_agent(&self, agent_id: &str) -> Result<()> {
        self.call(Operation::InstallAgent {
            agent_id: agent_id.to_owned(),
        })
        .await
    }
    pub async fn installation_status(&self, agent_id: &str) -> Result<Option<ConnectionStatus>> {
        self.call(Operation::InstallationStatus {
            agent_id: agent_id.to_owned(),
        })
        .await
    }
    pub async fn connect(&self, agent_id: &str, cwd: PathBuf) -> Result<ConnectionInfo> {
        self.call(Operation::Connect {
            agent_id: agent_id.to_owned(),
            cwd,
        })
        .await
    }
    pub async fn connection_status(
        &self,
        agent_id: &str,
        cwd: PathBuf,
    ) -> Result<Option<ConnectionStatus>> {
        self.call(Operation::ConnectionStatus {
            agent_id: agent_id.to_owned(),
            cwd,
        })
        .await
    }
    pub async fn connection_requests(&self, connection_id: &str) -> Result<Vec<Permission>> {
        self.call(Operation::ConnectionRequests {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn disconnect(&self, connection_id: &str) -> Result<()> {
        self.call(Operation::Disconnect {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn remote_sessions(
        &self,
        connection_id: &str,
        cursor: Option<String>,
    ) -> Result<Value> {
        self.call(Operation::RemoteSessions {
            connection_id: connection_id.to_owned(),
            cursor,
        })
        .await
    }
    pub async fn import_sessions(
        &self,
        connection_id: &str,
        sessions: Vec<SessionImport>,
    ) -> Result<Vec<SessionInfo>> {
        self.call(Operation::ImportSessions {
            connection_id: connection_id.to_owned(),
            sessions,
        })
        .await
    }
    pub async fn new_session(&self, connection_id: &str) -> Result<SessionSnapshot> {
        self.call(Operation::NewSession {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn fork_session(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.call(Operation::ForkSession {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn select_agent_id(&self, explicit: Option<&str>) -> Result<String> {
        self.call(Operation::SelectAgentId {
            explicit: explicit.map(str::to_owned),
        })
        .await
    }
    pub async fn prepare_session(&self, agent_id: &str, cwd: PathBuf) -> Result<SessionSnapshot> {
        self.call(Operation::PrepareSession {
            agent_id: agent_id.to_owned(),
            cwd,
        })
        .await
    }
    pub async fn prepare_session_with_options(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        creation: SessionCreation,
    ) -> Result<SessionSnapshot> {
        self.call(Operation::PrepareSessionWithOptions {
            agent_id: agent_id.to_owned(),
            cwd,
            creation,
        })
        .await
    }
    pub async fn session_info(&self, thread_id: &str) -> Result<SessionInfo> {
        self.call(Operation::SessionInfo {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn session_connection_info(&self, thread_id: &str) -> Result<Option<ConnectionInfo>> {
        self.call(Operation::SessionConnectionInfo {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn thread_for_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<String> {
        self.call(Operation::ThreadForSession {
            connection_id: connection_id.to_owned(),
            session_id: session_id.to_owned(),
        })
        .await
    }
    pub async fn load_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<SessionSnapshot> {
        self.call(Operation::LoadSession {
            connection_id: connection_id.to_owned(),
            session_id: session_id.to_owned(),
        })
        .await
    }
    pub async fn resume(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.call(Operation::Resume {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn sessions(&self, cwd: Option<&str>) -> Result<Vec<SessionInfo>> {
        self.call(Operation::Sessions {
            cwd: cwd.map(str::to_owned),
        })
        .await
    }
    pub async fn snapshot(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.call(Operation::Snapshot {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn overview(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.call(Operation::Overview {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn overview_updates(
        &self,
        thread_id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<SessionUpdates> {
        self.call(Operation::OverviewUpdates {
            thread_id: thread_id.to_owned(),
            generation: generation.to_owned(),
            revision,
        })
        .await
    }
    pub async fn process(
        &self,
        thread_id: &str,
        turn_id: &str,
        generation: Option<&str>,
        revision: Option<u64>,
    ) -> Result<SessionUpdates> {
        self.call(Operation::Process {
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.to_owned(),
            generation: generation.map(str::to_owned),
            revision,
        })
        .await
    }
    pub async fn updates(
        &self,
        thread_id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<SessionUpdates> {
        self.call(Operation::Updates {
            thread_id: thread_id.to_owned(),
            generation: generation.to_owned(),
            revision,
        })
        .await
    }
    pub async fn logs(&self, connection_id: &str) -> Result<Vec<Value>> {
        self.call(Operation::Logs {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn session_logs(&self, thread_id: &str) -> Result<Vec<Value>> {
        self.call(Operation::SessionLogs {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn dismiss_notice(&self, thread_id: &str, notice_id: &str) -> Result<()> {
        self.call(Operation::DismissNotice {
            thread_id: thread_id.to_owned(),
            notice_id: notice_id.to_owned(),
        })
        .await
    }
    pub async fn clear_plan(&self, thread_id: &str) -> Result<()> {
        self.call(Operation::ClearPlan {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn close_session(&self, thread_id: &str) -> Result<()> {
        self.call(Operation::CloseSession {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn set_workspace_removing(&self, cwd: &str, removing: bool) -> Result<()> {
        self.call(Operation::SetWorkspaceRemoving {
            cwd: cwd.to_owned(),
            removing,
        })
        .await
    }
    pub async fn recover_workspace_removals(&self) -> Result<()> {
        self.call(Operation::RecoverWorkspaceRemovals).await
    }
    pub async fn delete_workspace(&self, cwd: &str) -> Result<usize> {
        self.call(Operation::DeleteWorkspace {
            cwd: cwd.to_owned(),
        })
        .await
    }
    pub async fn delete_session(&self, thread_id: &str) -> Result<()> {
        self.call(Operation::DeleteSession {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
}
