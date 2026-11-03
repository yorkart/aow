use anyhow::Result;
use aow_acp::*;
use aow_acpd_client::protocol::Request;
use serde_json::{Value, to_value};

pub(super) async fn dispatch(
    service: &AcpService,
    host: std::sync::Arc<crate::host::Host>,
    request: Request,
) -> Result<Value> {
    match request {
        Request::RecoverWorkspaceRemovals {} => {
            service.recover_workspace_removals().await?;
            host.filesystem.write().await.clear();
            Ok(Value::Null)
        }
        Request::Probe {
            agent_id,
            cwd,
            execution,
        } => {
            crate::probe::run(host, agent_id, cwd, execution).await?;
            Ok(Value::Null)
        }
        Request::SyncEvent { resource } => Ok(to_value(service.sync_event(&resource))?),
        Request::Agents {} => Ok(to_value(service.agents().await?)?),
        Request::Registry { refresh } => Ok(to_value(service.registry(refresh).await?)?),
        Request::InstallAgent { agent_id } => {
            Ok(to_value(service.install_agent(&agent_id).await?)?)
        }
        Request::InstallationStatus { agent_id } => {
            Ok(to_value(service.installation_status(&agent_id))?)
        }
        Request::Connect { agent_id, cwd } => Ok(to_value(service.connect(&agent_id, cwd).await?)?),
        Request::ConnectionStatus { agent_id, cwd } => {
            Ok(to_value(service.connection_status(&agent_id, cwd).await?)?)
        }
        Request::ConnectionRequests { connection_id } => {
            Ok(to_value(service.connection_requests(&connection_id)?)?)
        }
        Request::AnswerConnection {
            connection_id,
            request_id,
            response,
        } => Ok(to_value(service.answer_connection(
            &connection_id,
            &request_id,
            response,
        )?)?),
        Request::Disconnect { connection_id } => Ok(to_value(service.disconnect(&connection_id)?)?),
        Request::Authenticate {
            connection_id,
            method_id,
        } => Ok(to_value(
            service.authenticate(&connection_id, &method_id).await?,
        )?),
        Request::RemoteSessions {
            connection_id,
            cursor,
        } => Ok(to_value(
            service.remote_sessions(&connection_id, cursor).await?,
        )?),
        Request::ImportSessions {
            connection_id,
            sessions,
        } => Ok(to_value(
            service.import_sessions(&connection_id, sessions)?,
        )?),
        Request::NewSession { connection_id } => {
            Ok(to_value(service.new_session(&connection_id).await?)?)
        }
        Request::ForkSession { thread_id } => {
            Ok(to_value(service.fork_session(&thread_id).await?)?)
        }
        Request::SelectAgentId { explicit } => Ok(to_value(
            service.select_agent_id(explicit.as_deref()).await?,
        )?),
        Request::PrepareSession { agent_id, cwd } => {
            Ok(to_value(service.prepare_session(&agent_id, cwd).await?)?)
        }
        Request::PrepareSessionWithOptions {
            agent_id,
            cwd,
            creation,
        } => Ok(to_value(
            service
                .prepare_session_with_options(&agent_id, cwd, creation)
                .await?,
        )?),
        Request::SessionInfo { thread_id } => Ok(to_value(service.session_info(&thread_id)?)?),
        Request::SessionConnectionInfo { thread_id } => {
            Ok(to_value(service.session_connection_info(&thread_id)?)?)
        }
        Request::ThreadForSession {
            connection_id,
            session_id,
        } => Ok(to_value(
            service.thread_for_session(&connection_id, &session_id)?,
        )?),
        Request::LoadSession {
            connection_id,
            session_id,
        } => Ok(to_value(
            service.load_session(&connection_id, &session_id).await?,
        )?),
        Request::Resume { thread_id } => Ok(to_value(service.resume(&thread_id).await?)?),
        Request::Sessions { cwd } => Ok(to_value(service.sessions(cwd.as_deref()))?),
        Request::Snapshot { thread_id } => Ok(to_value(service.snapshot(&thread_id)?)?),
        Request::Overview { thread_id } => Ok(to_value(service.overview(&thread_id)?)?),
        Request::OverviewUpdates {
            thread_id,
            generation,
            revision,
        } => Ok(to_value(service.overview_updates(
            &thread_id,
            &generation,
            revision,
        )?)?),
        Request::Process {
            thread_id,
            turn_id,
            generation,
            revision,
        } => Ok(to_value(service.process(
            &thread_id,
            &turn_id,
            generation.as_deref(),
            revision,
        )?)?),
        Request::Updates {
            thread_id,
            generation,
            revision,
        } => Ok(to_value(service.updates(
            &thread_id,
            &generation,
            revision,
        )?)?),
        Request::Logs { connection_id } => Ok(to_value(service.logs(&connection_id)?)?),
        Request::SessionLogs { thread_id } => Ok(to_value(service.session_logs(&thread_id)?)?),
        Request::StartPrompt { thread_id, content } => {
            Ok(to_value(service.start_prompt(&thread_id, content)?)?)
        }
        Request::StartPromptOnce {
            thread_id,
            request_id,
            content,
        } => Ok(to_value(service.start_prompt_once(
            &thread_id,
            &request_id,
            content,
        )?)?),
        Request::Cancel { thread_id } => Ok(to_value(service.cancel(&thread_id)?)?),
        Request::StartSteering { thread_id, content } => {
            Ok(to_value(service.start_steering(&thread_id, content)?)?)
        }
        Request::DismissNotice {
            thread_id,
            notice_id,
        } => Ok(to_value(service.dismiss_notice(&thread_id, &notice_id)?)?),
        Request::ClearPlan { thread_id } => Ok(to_value(service.clear_plan(&thread_id)?)?),
        Request::SetMode { thread_id, mode } => {
            Ok(to_value(service.set_mode(&thread_id, &mode).await?)?)
        }
        Request::SetConfigOption {
            thread_id,
            option,
            value,
        } => Ok(to_value(
            service
                .set_config_option(&thread_id, &option, value)
                .await?,
        )?),
        Request::Answer {
            thread_id,
            request,
            response,
        } => Ok(to_value(service.answer(&thread_id, &request, response)?)?),
        Request::CloseSession { thread_id } => {
            Ok(to_value(service.close_session(&thread_id).await?)?)
        }
        Request::SetWorkspaceRemoving { cwd, removing } => {
            if removing {
                service.set_workspace_removing(&cwd, true);
            }
            host.set_workspace_removing(&cwd, removing).await;
            if !removing {
                service.set_workspace_removing(&cwd, false);
            }
            Ok(Value::Null)
        }
        Request::DeleteWorkspace { cwd } => {
            service.set_workspace_removing(&cwd, true);
            host.set_workspace_removing(&cwd, true).await;
            Ok(to_value(service.delete_workspace(&cwd).await?)?)
        }
        Request::DeleteSession { thread_id } => {
            Ok(to_value(service.delete_session(&thread_id).await?)?)
        }
    }
}
