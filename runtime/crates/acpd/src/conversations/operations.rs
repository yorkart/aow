use anyhow::Result;
use aow_acp::AcpService;
use aow_runtime_contract::acp::Operation;
use serde_json::Value;

pub(super) async fn dispatch(
    service: &AcpService,
    operation: Operation,
    stopping: tokio::sync::watch::Receiver<bool>,
) -> Result<Value> {
    let value = match operation {
        Operation::SyncEvent { resource } => serde_json::to_value(service.sync_event(&resource))?,
        Operation::Agents => serde_json::to_value(service.agents().await?)?,
        Operation::Registry { refresh } => serde_json::to_value(service.registry(refresh).await?)?,
        Operation::InstallAgent { agent_id } => {
            serde_json::to_value(service.install_agent(&agent_id).await?)?
        }
        Operation::InstallationStatus { agent_id } => {
            serde_json::to_value(service.installation_status(&agent_id))?
        }
        Operation::Connect { agent_id, cwd } => {
            serde_json::to_value(service.connect(&agent_id, cwd).await?)?
        }
        Operation::ConnectionStatus { agent_id, cwd } => {
            serde_json::to_value(service.connection_status(&agent_id, cwd).await?)?
        }
        Operation::ConnectionRequests { connection_id } => {
            serde_json::to_value(service.connection_requests(&connection_id)?)?
        }
        Operation::Disconnect { connection_id } => {
            serde_json::to_value(service.disconnect(&connection_id)?)?
        }
        Operation::RemoteSessions {
            connection_id,
            cursor,
        } => serde_json::to_value(service.remote_sessions(&connection_id, cursor).await?)?,
        Operation::ImportSessions {
            connection_id,
            sessions,
        } => serde_json::to_value(service.import_sessions(&connection_id, sessions)?)?,
        Operation::NewSession { connection_id } => {
            serde_json::to_value(service.new_session(&connection_id).await?)?
        }
        Operation::ForkSession { thread_id } => {
            serde_json::to_value(service.fork_session(&thread_id).await?)?
        }
        Operation::SelectAgentId { explicit } => {
            serde_json::to_value(service.select_agent_id(explicit.as_deref()).await?)?
        }
        Operation::PrepareSession { agent_id, cwd } => {
            serde_json::to_value(service.prepare_session(&agent_id, cwd).await?)?
        }
        Operation::PrepareSessionWithOptions {
            agent_id,
            cwd,
            creation,
        } => serde_json::to_value(
            service
                .prepare_session_with_options(&agent_id, cwd, creation)
                .await?,
        )?,
        Operation::SessionInfo { thread_id } => {
            serde_json::to_value(service.session_info(&thread_id)?)?
        }
        Operation::SessionConnectionInfo { thread_id } => {
            serde_json::to_value(service.session_connection_info(&thread_id)?)?
        }
        Operation::ThreadForSession {
            connection_id,
            session_id,
        } => serde_json::to_value(service.thread_for_session(&connection_id, &session_id)?)?,
        Operation::LoadSession {
            connection_id,
            session_id,
        } => serde_json::to_value(service.load_session(&connection_id, &session_id).await?)?,
        Operation::Resume { thread_id } => serde_json::to_value(service.resume(&thread_id).await?)?,
        Operation::Sessions { cwd } => serde_json::to_value(service.sessions(cwd.as_deref()))?,
        Operation::Snapshot { thread_id } => serde_json::to_value(service.snapshot(&thread_id)?)?,
        Operation::Overview { thread_id } => serde_json::to_value(service.overview(&thread_id)?)?,
        Operation::OverviewUpdates {
            thread_id,
            generation,
            revision,
        } => serde_json::to_value(service.overview_updates(&thread_id, &generation, revision)?)?,
        Operation::Process {
            thread_id,
            turn_id,
            generation,
            revision,
        } => serde_json::to_value(service.process(
            &thread_id,
            &turn_id,
            generation.as_deref(),
            revision,
        )?)?,
        Operation::Updates {
            thread_id,
            generation,
            revision,
        } => serde_json::to_value(service.updates(&thread_id, &generation, revision)?)?,
        Operation::Logs { connection_id } => serde_json::to_value(service.logs(&connection_id)?)?,
        Operation::SessionLogs { thread_id } => {
            serde_json::to_value(service.session_logs(&thread_id)?)?
        }
        Operation::DismissNotice {
            thread_id,
            notice_id,
        } => serde_json::to_value(service.dismiss_notice(&thread_id, &notice_id)?)?,
        Operation::ClearPlan { thread_id } => {
            serde_json::to_value(service.clear_plan(&thread_id)?)?
        }
        Operation::CloseSession { thread_id } => {
            serde_json::to_value(service.close_session(&thread_id).await?)?
        }
        Operation::SetWorkspaceRemoving { cwd, removing } => {
            serde_json::to_value(service.set_workspace_removing(&cwd, removing))?
        }
        Operation::RecoverWorkspaceRemovals => {
            serde_json::to_value(service.recover_workspace_removals().await?)?
        }
        Operation::DeleteWorkspace { cwd } => {
            serde_json::to_value(service.delete_workspace(&cwd).await?)?
        }
        Operation::DeleteSession { thread_id } => {
            serde_json::to_value(service.delete_session(&thread_id).await?)?
        }
        Operation::Probe {
            agent_id,
            cwd,
            execution,
        } => {
            crate::runtime::AcpProber::new(service.clone(), stopping)
                .probe(&agent_id, cwd, execution)
                .await?;
            Value::Null
        }
        Operation::Checkpoint => {
            service.flush_threads_on_quit()?;
            Value::Null
        }
    };
    Ok(value)
}
