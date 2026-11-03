use super::{AgentRole, RoleRuntime};
use crate::{
    AppState, HttpError,
    aow::{AgentLaunch, AowError, AowManager, aow_http_error},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RoleSession {
    Acp {
        session: Box<aow_acp::SessionSnapshot>,
    },
    Cli {
        terminal: Box<aow_protocol::TerminalTab>,
        #[serde(skip)]
        instruction: Option<String>,
        #[serde(skip)]
        prompt_in_arguments: bool,
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
                instruction,
                prompt_in_arguments,
            } => {
                state.workspace_events.terminals_changed();
                if *prompt_in_arguments {
                    return Ok(());
                }
                let prompt = match (instruction, prompt) {
                    (Some(instruction), Some(prompt)) => Some(format!("{instruction}\n\n{prompt}")),
                    (Some(instruction), None) => Some(instruction.clone()),
                    (None, prompt) => prompt,
                };
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

impl AowManager {
    pub(crate) async fn role_cli_launch(&self, role: &AgentRole) -> Result<AgentLaunch, AowError> {
        let RoleRuntime::Cli { agent_id } = &role.config.runtime else {
            return Err(AowError::Invalid("This role uses ACP".into()));
        };
        let mut launch = self.role_runtime_cli_launch(agent_id).await?;
        if super::runtime::layout_for(launch.agent_type.id()) != role.config.home_layout {
            return Err(AowError::Invalid("The referenced runtime's agent type changed; select a runtime matching this role's identity".into()));
        }
        launch.display_name = role.config.name.clone();
        launch.env.extend(self.role_environment(role)?);
        if let Some(state) = &self.inner.state_dir {
            launch
                .env
                .insert("AOW_STATE_DIR".into(), state.to_string_lossy().into_owned());
        }
        if !role.config.instruction.trim().is_empty() {
            let extra = match launch.agent_type {
                aow_agents::launch::AgentType::Codex => vec![
                    "-c".into(),
                    format!(
                        "developer_instructions={}",
                        serde_json::to_string(&role.config.instruction)?
                    ),
                ],
                aow_agents::launch::AgentType::Claude | aow_agents::launch::AgentType::Pi => vec![
                    "--append-system-prompt".into(),
                    role.config.instruction.clone(),
                ],
                _ => vec![],
            };
            let index = launch
                .args
                .iter()
                .position(|arg| arg == "--")
                .unwrap_or(launch.args.len());
            launch.args.splice(index..index, extra);
        }
        Ok(launch)
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
                self.role_environment(role).map_err(aow_http_error)?;
            }
            RoleRuntime::Cli { .. } => {
                self.role_cli_launch(role).await.map_err(aow_http_error)?;
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
        parent: Option<aow_protocol::InstanceRef>,
    ) -> Result<RoleSession, HttpError> {
        self.check_role_runtime(role, state).await?;
        match &role.config.runtime {
            RoleRuntime::Acp { agent_id } => {
                let session = state
                    .acp
                    .prepare_session_with_options(
                        agent_id,
                        cwd.into(),
                        aow_acp::SessionCreation {
                            parent,
                            execution_environment: Some(
                                self.role_acp_environment(role).map_err(aow_http_error)?,
                            ),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(crate::acp::task_error)?;
                if let Err(error) = self.bind_role_acp_session(&session.thread_id, role) {
                    let _ = state.acp.delete_session(&session.thread_id).await;
                    return Err(aow_http_error(error));
                }
                Ok(RoleSession::Acp {
                    session: Box::new(session),
                })
            }
            RoleRuntime::Cli { .. } => {
                let launch = self.role_cli_launch(role).await.map_err(aow_http_error)?;
                // Claude accepts the initial prompt as a positional argument and
                // has no interactive startup adapter. Keep its terminal usable
                // for native onboarding and account selection as usual.
                let prompt_in_arguments =
                    launch.agent_type == aow_agents::launch::AgentType::Claude;
                let mut initial_arguments = Vec::new();
                if prompt_in_arguments && let Some(prompt) = prompt {
                    if !launch.args.iter().any(|arg| arg == "--") {
                        initial_arguments.push("--".into());
                    }
                    initial_arguments.push(prompt.into());
                }
                let instruction = matches!(
                    launch.agent_type,
                    aow_agents::launch::AgentType::Hermes | aow_agents::launch::AgentType::TraeCli
                )
                .then(|| role.config.instruction.clone())
                .filter(|text| !text.trim().is_empty());
                let terminal = state
                    .terminals
                    .create_role_agent(
                        &role.id,
                        cwd,
                        launch,
                        &initial_arguments,
                        !prompt_in_arguments && (prompt.is_some() || instruction.is_some()),
                    )
                    .await
                    .map_err(crate::terminal::terminal_http_error)?;
                Ok(RoleSession::Cli {
                    terminal: Box::new(terminal),
                    instruction,
                    prompt_in_arguments,
                })
            }
        }
    }
}
