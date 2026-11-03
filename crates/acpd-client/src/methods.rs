use super::{AcpClient, client::Backend, protocol::Request};
use anyhow::Result;
use aow_acp::*;
use serde_json::Value;
use std::path::PathBuf;

impl AcpClient {
    pub async fn recover_workspace_removals(&self) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.recover_workspace_removals().await;
        }
        self.call(Request::RecoverWorkspaceRemovals {}).await
    }

    pub async fn sync_event(&self, resource: &SyncResource) -> Result<SyncEvent> {
        if let Backend::Embedded(service) = &self.backend {
            return Ok(service.sync_event(resource));
        }
        self.call(Request::SyncEvent {
            resource: resource.clone(),
        })
        .await
    }
    pub async fn agents(&self) -> Result<Vec<AgentInfo>> {
        if let Backend::Embedded(service) = &self.backend {
            return service.agents().await;
        }
        self.call(Request::Agents {}).await
    }
    pub async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        if let Backend::Embedded(service) = &self.backend {
            return service.registry(refresh).await;
        }
        self.call(Request::Registry { refresh }).await
    }
    pub async fn install_agent(&self, agent_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.install_agent(agent_id).await;
        }
        self.call(Request::InstallAgent {
            agent_id: agent_id.to_owned(),
        })
        .await
    }
    pub async fn installation_status(&self, agent_id: &str) -> Result<Option<ConnectionStatus>> {
        if let Backend::Embedded(service) = &self.backend {
            return Ok(service.installation_status(agent_id));
        }
        self.call(Request::InstallationStatus {
            agent_id: agent_id.to_owned(),
        })
        .await
    }
    pub async fn connect(&self, agent_id: &str, cwd: PathBuf) -> Result<ConnectionInfo> {
        if let Backend::Embedded(service) = &self.backend {
            return service.connect(agent_id, cwd).await;
        }
        self.call(Request::Connect {
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
        if let Backend::Embedded(service) = &self.backend {
            return service.connection_status(agent_id, cwd).await;
        }
        self.call(Request::ConnectionStatus {
            agent_id: agent_id.to_owned(),
            cwd,
        })
        .await
    }
    pub async fn connection_requests(&self, connection_id: &str) -> Result<Vec<Permission>> {
        if let Backend::Embedded(service) = &self.backend {
            return service.connection_requests(connection_id);
        }
        self.call(Request::ConnectionRequests {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn answer_connection(
        &self,
        connection_id: &str,
        request_id: &str,
        response: Value,
    ) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.answer_connection(connection_id, request_id, response);
        }
        self.call(Request::AnswerConnection {
            connection_id: connection_id.to_owned(),
            request_id: request_id.to_owned(),
            response,
        })
        .await
    }
    pub async fn disconnect(&self, connection_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.disconnect(connection_id);
        }
        self.call(Request::Disconnect {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn authenticate(&self, connection_id: &str, method_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.authenticate(connection_id, method_id).await;
        }
        self.call(Request::Authenticate {
            connection_id: connection_id.to_owned(),
            method_id: method_id.to_owned(),
        })
        .await
    }
    pub async fn remote_sessions(
        &self,
        connection_id: &str,
        cursor: Option<String>,
    ) -> Result<Value> {
        if let Backend::Embedded(service) = &self.backend {
            return service.remote_sessions(connection_id, cursor).await;
        }
        self.call(Request::RemoteSessions {
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
        if let Backend::Embedded(service) = &self.backend {
            return service.import_sessions(connection_id, sessions);
        }
        self.call(Request::ImportSessions {
            connection_id: connection_id.to_owned(),
            sessions,
        })
        .await
    }
    pub async fn new_session(&self, connection_id: &str) -> Result<SessionSnapshot> {
        if let Backend::Embedded(service) = &self.backend {
            return service.new_session(connection_id).await;
        }
        self.call(Request::NewSession {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn fork_session(&self, thread_id: &str) -> Result<SessionSnapshot> {
        if let Backend::Embedded(service) = &self.backend {
            return service.fork_session(thread_id).await;
        }
        self.call(Request::ForkSession {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn select_agent_id(&self, explicit: Option<&str>) -> Result<String> {
        if let Backend::Embedded(service) = &self.backend {
            return service.select_agent_id(explicit).await;
        }
        self.call(Request::SelectAgentId {
            explicit: explicit.map(str::to_owned),
        })
        .await
    }
    pub async fn prepare_session(&self, agent_id: &str, cwd: PathBuf) -> Result<SessionSnapshot> {
        if let Backend::Embedded(service) = &self.backend {
            return service.prepare_session(agent_id, cwd).await;
        }
        self.call(Request::PrepareSession {
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
        if let Backend::Embedded(service) = &self.backend {
            return service
                .prepare_session_with_options(agent_id, cwd, creation)
                .await;
        }
        self.call(Request::PrepareSessionWithOptions {
            agent_id: agent_id.to_owned(),
            cwd,
            creation,
        })
        .await
    }
    pub async fn session_info(&self, thread_id: &str) -> Result<SessionInfo> {
        if let Backend::Embedded(service) = &self.backend {
            return service.session_info(thread_id);
        }
        self.call(Request::SessionInfo {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn session_connection_info(&self, thread_id: &str) -> Result<Option<ConnectionInfo>> {
        if let Backend::Embedded(service) = &self.backend {
            return service.session_connection_info(thread_id);
        }
        self.call(Request::SessionConnectionInfo {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn thread_for_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<String> {
        if let Backend::Embedded(service) = &self.backend {
            return service.thread_for_session(connection_id, session_id);
        }
        self.call(Request::ThreadForSession {
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
        if let Backend::Embedded(service) = &self.backend {
            return service.load_session(connection_id, session_id).await;
        }
        self.call(Request::LoadSession {
            connection_id: connection_id.to_owned(),
            session_id: session_id.to_owned(),
        })
        .await
    }
    pub async fn resume(&self, thread_id: &str) -> Result<SessionSnapshot> {
        if let Backend::Embedded(service) = &self.backend {
            return service.resume(thread_id).await;
        }
        self.call(Request::Resume {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn sessions(&self, cwd: Option<&str>) -> Result<Vec<SessionInfo>> {
        if let Backend::Embedded(service) = &self.backend {
            return Ok(service.sessions(cwd));
        }
        self.call(Request::Sessions {
            cwd: cwd.map(str::to_owned),
        })
        .await
    }
    pub async fn snapshot(&self, thread_id: &str) -> Result<SessionSnapshot> {
        if let Backend::Embedded(service) = &self.backend {
            return service.snapshot(thread_id);
        }
        self.call(Request::Snapshot {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn overview(&self, thread_id: &str) -> Result<SessionSnapshot> {
        if let Backend::Embedded(service) = &self.backend {
            return service.overview(thread_id);
        }
        self.call(Request::Overview {
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
        if let Backend::Embedded(service) = &self.backend {
            return service.overview_updates(thread_id, generation, revision);
        }
        self.call(Request::OverviewUpdates {
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
        if let Backend::Embedded(service) = &self.backend {
            return service.process(thread_id, turn_id, generation, revision);
        }
        self.call(Request::Process {
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
        if let Backend::Embedded(service) = &self.backend {
            return service.updates(thread_id, generation, revision);
        }
        self.call(Request::Updates {
            thread_id: thread_id.to_owned(),
            generation: generation.to_owned(),
            revision,
        })
        .await
    }
    pub async fn logs(&self, connection_id: &str) -> Result<Vec<Value>> {
        if let Backend::Embedded(service) = &self.backend {
            return service.logs(connection_id);
        }
        self.call(Request::Logs {
            connection_id: connection_id.to_owned(),
        })
        .await
    }
    pub async fn session_logs(&self, thread_id: &str) -> Result<Vec<Value>> {
        if let Backend::Embedded(service) = &self.backend {
            return service.session_logs(thread_id);
        }
        self.call(Request::SessionLogs {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn start_prompt(&self, thread_id: &str, content: Vec<Content>) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.start_prompt(thread_id, content);
        }
        self.call(Request::StartPrompt {
            thread_id: thread_id.to_owned(),
            content,
        })
        .await
    }
    pub async fn start_prompt_once(
        &self,
        thread_id: &str,
        request_id: &str,
        content: Vec<Content>,
    ) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.start_prompt_once(thread_id, request_id, content);
        }
        self.call(Request::StartPromptOnce {
            thread_id: thread_id.to_owned(),
            request_id: request_id.to_owned(),
            content,
        })
        .await
    }
    pub async fn cancel(&self, thread_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.cancel(thread_id);
        }
        self.call(Request::Cancel {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn start_steering(&self, thread_id: &str, content: Vec<Content>) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.start_steering(thread_id, content);
        }
        self.call(Request::StartSteering {
            thread_id: thread_id.to_owned(),
            content,
        })
        .await
    }
    pub async fn dismiss_notice(&self, thread_id: &str, notice_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.dismiss_notice(thread_id, notice_id);
        }
        self.call(Request::DismissNotice {
            thread_id: thread_id.to_owned(),
            notice_id: notice_id.to_owned(),
        })
        .await
    }
    pub async fn clear_plan(&self, thread_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.clear_plan(thread_id);
        }
        self.call(Request::ClearPlan {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn set_mode(&self, thread_id: &str, mode: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.set_mode(thread_id, mode).await;
        }
        self.call(Request::SetMode {
            thread_id: thread_id.to_owned(),
            mode: mode.to_owned(),
        })
        .await
    }
    pub async fn set_config_option(
        &self,
        thread_id: &str,
        option: &str,
        value: Value,
    ) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.set_config_option(thread_id, option, value).await;
        }
        self.call(Request::SetConfigOption {
            thread_id: thread_id.to_owned(),
            option: option.to_owned(),
            value,
        })
        .await
    }
    pub async fn answer(&self, thread_id: &str, request: &str, response: Value) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.answer(thread_id, request, response);
        }
        self.call(Request::Answer {
            thread_id: thread_id.to_owned(),
            request: request.to_owned(),
            response,
        })
        .await
    }
    pub async fn close_session(&self, thread_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.close_session(thread_id).await;
        }
        self.call(Request::CloseSession {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
    pub async fn set_workspace_removing(&self, cwd: &str, removing: bool) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            service.set_workspace_removing(cwd, removing);
            return Ok(());
        }
        self.call(Request::SetWorkspaceRemoving {
            cwd: cwd.to_owned(),
            removing,
        })
        .await
    }
    pub async fn delete_workspace(&self, cwd: &str) -> Result<usize> {
        if let Backend::Embedded(service) = &self.backend {
            return service.delete_workspace(cwd).await;
        }
        self.call(Request::DeleteWorkspace {
            cwd: cwd.to_owned(),
        })
        .await
    }
    pub async fn delete_session(&self, thread_id: &str) -> Result<()> {
        if let Backend::Embedded(service) = &self.backend {
            return service.delete_session(thread_id).await;
        }
        self.call(Request::DeleteSession {
            thread_id: thread_id.to_owned(),
        })
        .await
    }
}
