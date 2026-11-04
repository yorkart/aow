use super::AcpClient;
use anyhow::Result;
use aow_acp_client::*;
use serde_json::Value;
use std::path::PathBuf;
impl AcpClient {
    pub async fn recover_workspace_removals(&self) -> Result<()> {
        if let Some(runtime) = &self.runtime {
            runtime.acp_ready().await?;
        }
        self.service.recover_workspace_removals().await
    }
    pub async fn sync_event(&self, resource: &SyncResource) -> Result<SyncEvent> {
        Ok(self.service.sync_event(resource))
    }
    pub async fn agents(&self) -> Result<Vec<AgentInfo>> {
        if let Some(runtime) = &self.runtime {
            let value: serde_json::Value = runtime.get_json("/v1/acp/agents").await?;
            return Ok(serde_json::from_value(value["agents"].clone())?);
        }

        self.service.agents().await
    }
    pub async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        if let Some(runtime) = &self.runtime {
            let value: serde_json::Value = runtime
                .get_json(&format!("/v1/acp/registry?refresh={refresh}"))
                .await?;
            return Ok(serde_json::from_value(value["agents"].clone())?);
        }

        self.service.registry(refresh).await
    }
    pub async fn install_agent(&self, agent_id: &str) -> Result<()> {
        if let Some(runtime) = &self.runtime {
            let _: serde_json::Value = runtime
                .post_json(
                    &format!("/v1/acp/agents/{agent_id}/install"),
                    &serde_json::json!({}),
                )
                .await?;
            return Ok(());
        }

        self.service.install_agent(agent_id).await
    }
    pub async fn installation_status(&self, agent_id: &str) -> Result<Option<ConnectionStatus>> {
        if let Some(runtime) = &self.runtime {
            return runtime
                .get_json(&format!("/v1/acp/agents/{agent_id}/installation"))
                .await
                .map_err(Into::into);
        }

        self.service.installation_status(agent_id).await
    }
    pub async fn connect(&self, agent_id: &str, cwd: PathBuf) -> Result<ConnectionInfo> {
        self.service.connect(agent_id, cwd).await
    }
    pub async fn connection_status(
        &self,
        agent_id: &str,
        cwd: PathBuf,
    ) -> Result<Option<ConnectionStatus>> {
        self.service.connection_status(agent_id, cwd).await
    }
    pub async fn connection_requests(&self, connection_id: &str) -> Result<Vec<Permission>> {
        self.service.connection_requests(connection_id)
    }
    pub async fn answer_connection(
        &self,
        connection_id: &str,
        request_id: &str,
        response: Value,
    ) -> Result<()> {
        self.service
            .answer_connection(connection_id, request_id, response)
    }
    pub async fn disconnect(&self, connection_id: &str) -> Result<()> {
        self.service.disconnect(connection_id)
    }
    pub async fn authenticate(&self, connection_id: &str, method_id: &str) -> Result<()> {
        self.service.authenticate(connection_id, method_id).await
    }
    pub async fn remote_sessions(
        &self,
        connection_id: &str,
        cursor: Option<String>,
    ) -> Result<Value> {
        self.service.remote_sessions(connection_id, cursor).await
    }
    pub async fn import_sessions(
        &self,
        connection_id: &str,
        sessions: Vec<SessionImport>,
    ) -> Result<Vec<SessionInfo>> {
        self.service.import_sessions(connection_id, sessions)
    }
    pub async fn new_session(&self, connection_id: &str) -> Result<SessionSnapshot> {
        self.service.new_session(connection_id).await
    }
    pub async fn fork_session(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.service.fork_session(thread_id).await
    }
    pub async fn select_agent_id(&self, explicit: Option<&str>) -> Result<String> {
        self.service.select_agent_id(explicit).await
    }
    pub async fn prepare_session(&self, agent_id: &str, cwd: PathBuf) -> Result<SessionSnapshot> {
        self.service.prepare_session(agent_id, cwd).await
    }
    pub async fn prepare_session_with_options(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        creation: SessionCreation,
    ) -> Result<SessionSnapshot> {
        self.service
            .prepare_session_with_options(agent_id, cwd, creation)
            .await
    }
    pub async fn session_info(&self, thread_id: &str) -> Result<SessionInfo> {
        self.service.session_info(thread_id)
    }
    pub async fn session_connection_info(&self, thread_id: &str) -> Result<Option<ConnectionInfo>> {
        self.service.session_connection_info(thread_id)
    }
    pub async fn thread_for_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<String> {
        self.service.thread_for_session(connection_id, session_id)
    }
    pub async fn load_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<SessionSnapshot> {
        self.service.load_session(connection_id, session_id).await
    }
    pub async fn resume(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.service.resume(thread_id).await
    }
    pub async fn sessions(&self, cwd: Option<&str>) -> Result<Vec<SessionInfo>> {
        Ok(self.service.sessions(cwd))
    }
    pub async fn snapshot(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.service.snapshot(thread_id)
    }
    pub async fn overview(&self, thread_id: &str) -> Result<SessionSnapshot> {
        self.service.overview(thread_id)
    }
    pub async fn overview_updates(
        &self,
        thread_id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<SessionUpdates> {
        self.service
            .overview_updates(thread_id, generation, revision)
    }
    pub async fn process(
        &self,
        thread_id: &str,
        turn_id: &str,
        generation: Option<&str>,
        revision: Option<u64>,
    ) -> Result<SessionUpdates> {
        self.service
            .process(thread_id, turn_id, generation, revision)
    }
    pub async fn updates(
        &self,
        thread_id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<SessionUpdates> {
        self.service.updates(thread_id, generation, revision)
    }
    pub async fn logs(&self, connection_id: &str) -> Result<Vec<Value>> {
        self.service.logs(connection_id)
    }
    pub async fn session_logs(&self, thread_id: &str) -> Result<Vec<Value>> {
        self.service.session_logs(thread_id)
    }
    pub async fn start_prompt(&self, thread_id: &str, content: Vec<Content>) -> Result<()> {
        self.service.start_prompt(thread_id, content)
    }
    pub async fn start_prompt_once(
        &self,
        thread_id: &str,
        request_id: &str,
        content: Vec<Content>,
    ) -> Result<()> {
        if let Some(runtime) = &self.runtime {
            runtime.health().await?;
        }

        self.service
            .start_prompt_once(thread_id, request_id, content)
    }
    pub async fn cancel(&self, thread_id: &str) -> Result<()> {
        self.service.cancel(thread_id)
    }
    pub async fn start_steering(&self, thread_id: &str, content: Vec<Content>) -> Result<()> {
        self.service.start_steering(thread_id, content)
    }
    pub async fn dismiss_notice(&self, thread_id: &str, notice_id: &str) -> Result<()> {
        self.service.dismiss_notice(thread_id, notice_id)
    }
    pub async fn clear_plan(&self, thread_id: &str) -> Result<()> {
        self.service.clear_plan(thread_id)
    }
    pub async fn set_mode(&self, thread_id: &str, mode: &str) -> Result<()> {
        self.service.set_mode(thread_id, mode).await
    }
    pub async fn set_config_option(
        &self,
        thread_id: &str,
        option: &str,
        value: Value,
    ) -> Result<()> {
        self.service
            .set_config_option(thread_id, option, value)
            .await
    }
    pub async fn answer(&self, thread_id: &str, request: &str, response: Value) -> Result<()> {
        self.service.answer(thread_id, request, response)
    }
    pub async fn close_session(&self, thread_id: &str) -> Result<()> {
        self.service.close_session(thread_id).await
    }
    pub async fn set_workspace_removing(&self, cwd: &str, removing: bool) -> Result<()> {
        if removing {
            self.service.set_workspace_removing(cwd, true);
        }
        if let Some(runtime) = &self.runtime {
            runtime.acp_ready().await?;
            let _: serde_json::Value = runtime
                .put_json(
                    "/v1/workspaces/removing",
                    &serde_json::json!({"cwd":cwd,"removing":removing}),
                )
                .await?;
        }
        self.service.set_workspace_removing(cwd, removing);
        Ok(())
    }
    pub async fn delete_workspace(&self, cwd: &str) -> Result<usize> {
        self.service.delete_workspace(cwd).await
    }
    pub async fn delete_session(&self, thread_id: &str) -> Result<()> {
        self.service.delete_session(thread_id).await
    }
}
