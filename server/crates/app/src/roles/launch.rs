use super::{AgentRole, RoleRuntime};
use crate::agents::AgentLaunch;
use crate::error::{ServiceError, service_http_error};
use crate::roles::RoleService;
use crate::{AppState, HttpError};
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RoleSession {
    Acp {
        session: Box<aow_host_client::acp::SessionSnapshot>,
    },
    Cli {
        terminal: Box<aow_api_contract::terminal::TerminalTab>,
        #[serde(skip)]
        task: Option<String>,
        #[serde(skip)]
        managed: bool,
    },
}

impl RoleSession {
    pub(crate) fn resource(&self) -> (&'static str, &str) {
        match self {
            Self::Acp { session } => ("acp", &session.thread_id),
            Self::Cli { terminal, .. } => ("terminal", &terminal.id),
        }
    }

    pub(crate) fn start(&self, state: &AppState, prompt: Option<String>) -> Result<(), HttpError> {
        match self {
            Self::Acp { session } => {
                crate::acp::start_task(state.clone(), session.thread_id.clone(), prompt)?;
            }
            Self::Cli {
                terminal,
                task,
                managed,
            } => {
                state.workspace_events.terminals_changed();
                if !*managed {
                    return Ok(());
                }
                let prompt = task.clone();
                state.terminals.start_role_agent(
                    terminal,
                    prompt,
                    state.workspace_events.clone(),
                )?;
            }
        }
        Ok(())
    }

    pub(crate) async fn discard(&self, state: &AppState) {
        match self {
            Self::Acp { session } => {
                let _ = state.acp.delete_session(&session.thread_id).await;
            }
            Self::Cli { terminal, .. } => {
                let _ = state.terminals.delete_tab(&terminal.id).await;
            }
        }
    }
}

impl RoleService {
    pub(crate) async fn role_cli_launch(
        &self,
        role: &AgentRole,
    ) -> Result<AgentLaunch, ServiceError> {
        let RoleRuntime::Cli { agent_id } = &role.config.runtime else {
            return Err(ServiceError::Invalid("This role uses ACP".into()));
        };
        let mut launch = self.role_runtime_cli_launch(agent_id).await?;
        if self.role_layout(launch.agent_type.id()).await? != role.config.home_layout {
            return Err(ServiceError::Invalid("The referenced runtime's agent type changed; select a runtime matching this role's identity".into()));
        }
        launch.display_name = role.config.name.clone();
        self.resolve_node_role(role).await?;
        launch.env.extend(self.role_environment(role)?);
        launch.env.extend(self.agents.callback_environment()?);
        self.node
            .agent_node()
            .await?
            .prepare_agent_role(&aow_host_client::agents::PrepareRole {
                launch,
                instruction: role.config.instruction.clone(),
            })
            .await
            .map_err(|e| ServiceError::Invalid(e.to_string()))
    }

    pub(crate) async fn check_role_runtime(
        &self,
        role: &AgentRole,
        state: &AppState,
    ) -> Result<(), HttpError> {
        match &role.config.runtime {
            RoleRuntime::Acp { agent_id } => {
                if !state
                    .acp
                    .agents()
                    .await
                    .map_err(crate::acp::task_error)?
                    .iter()
                    .any(|agent| {
                        agent.id == *agent_id
                            && agent.configured
                            && agent.supported
                            && agent.installed
                    })
                {
                    return Err(crate::instances::invalid(
                        "Install or configure the role's ACP runtime first",
                    ));
                }
                self.resolve_node_role(role)
                    .await
                    .map_err(service_http_error)?;
                self.role_environment(role).map_err(service_http_error)?;
            }
            RoleRuntime::Cli { .. } => {
                self.role_cli_launch(role)
                    .await
                    .map_err(service_http_error)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn prepare_role_session(
        &self,
        role: &AgentRole,
        state: &AppState,
        cwd: &str,
        prompt: Option<&str>,
        parent: Option<aow_host_contract::instance::InstanceRef>,
    ) -> Result<RoleSession, HttpError> {
        self.check_role_runtime(role, state).await?;
        match &role.config.runtime {
            RoleRuntime::Acp { agent_id } => {
                let session = state
                    .acp
                    .prepare_session_with_options(
                        agent_id,
                        cwd.into(),
                        aow_host_client::acp::SessionCreation {
                            parent,
                            execution_environment: Some(
                                self.role_acp_environment(role)
                                    .map_err(service_http_error)?,
                            ),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(crate::acp::task_error)?;
                if let Err(error) = self.bind_role_acp_session(&session.thread_id, role) {
                    let _ = state.acp.delete_session(&session.thread_id).await;
                    return Err(service_http_error(error));
                }
                Ok(RoleSession::Acp {
                    session: Box::new(session),
                })
            }
            RoleRuntime::Cli { .. } => {
                let launch = self
                    .role_cli_launch(role)
                    .await
                    .map_err(service_http_error)?;
                let plan = self
                    .node
                    .agent_node()
                    .await
                    .map_err(service_http_error)?
                    .prepare_role_session(&aow_host_client::agents::PrepareRoleSession {
                        launch,
                        instruction: role.config.instruction.clone(),
                        prompt: prompt.map(str::to_owned),
                    })
                    .await
                    .map_err(|e| service_http_error(ServiceError::Invalid(e.to_string())))?;
                let terminal = state
                    .terminals
                    .create_role_agent(
                        &role.id,
                        cwd,
                        plan.launch,
                        &plan.initial_arguments,
                        plan.managed,
                    )
                    .await
                    .map_err(crate::terminal::terminal_http_error)?;
                Ok(RoleSession::Cli {
                    terminal: Box::new(terminal),
                    task: plan.task,
                    managed: plan.managed,
                })
            }
        }
    }
}
