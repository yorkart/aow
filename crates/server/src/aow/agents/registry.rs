use std::{
    collections::{BTreeMap, HashSet},
    os::unix::fs::PermissionsExt,
};

use super::super::*;
use super::order::{group_agents, type_order};

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct RegisterAgentRequest {
    pub(in crate::aow) id: String,
    pub(in crate::aow) agent_type: AgentType,
    pub(in crate::aow) display_name: String,
    pub(in crate::aow) command: String,
    #[serde(default)]
    pub(in crate::aow) args: Vec<String>,
    #[serde(default)]
    pub(in crate::aow) env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
pub(in crate::aow) struct AgentsQuery {
    #[allow(dead_code)]
    pub(in crate::aow) refresh: Option<bool>,
}

pub(crate) fn resolve_executable(command: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let command_path = Path::new(command);
    if command_path.components().count() > 1 {
        return executable_file(command_path).then(|| command_path.to_path_buf());
    }
    paths
        .iter()
        .map(|directory| directory.join(command))
        .find(|candidate| executable_file(candidate))
}

fn executable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

impl AowManager {
    pub(crate) async fn select_agent_id(&self, explicit: Option<&str>) -> Result<String, AowError> {
        if let Some(id) = explicit {
            return Ok(id.to_owned());
        }
        let agents = self.agents().await?;
        agents
            .iter()
            .find(|agent| agent.is_default)
            .or_else(|| agents.first())
            .map(|agent| agent.id.clone())
            .ok_or_else(|| {
                AowError::Invalid(aow_i18n::text("server.no_agent_instances_configured").into())
            })
    }

    pub(in crate::aow) async fn set_default_agent(
        &self,
        id: Option<String>,
    ) -> Result<(), AowError> {
        let _operation = self.inner.agent_operation.lock().await;
        if let Some(id) = &id {
            validation::validate_id(id)?;
            if !self.agents().await?.iter().any(|agent| &agent.id == id) {
                return Err(AowError::AgentNotFound(id.clone()));
            }
        }
        let mut state = self.lock()?;
        self.persist_agents(&state.agents, id.as_deref())?;
        state.default_agent_id = id;
        Ok(())
    }

    pub(in crate::aow) async fn agents(&self) -> Result<Vec<AgentRegistration>, AowError> {
        let path = self.execution_path().await?;
        self.agents_in_path(&path)
    }

    pub(in crate::aow) fn agents_in_path(
        &self,
        path: &[PathBuf],
    ) -> Result<Vec<AgentRegistration>, AowError> {
        let (custom, default_agent_id) = {
            let state = self.lock()?;
            (state.agents.clone(), state.default_agent_id.clone())
        };
        let custom_ids = custom
            .iter()
            .map(|agent| agent.id.clone())
            .collect::<HashSet<_>>();
        let mut agents = Vec::new();
        for agent in custom {
            let agent_type = agent.agent_type.or_else(|| AgentType::from_id(&agent.id));
            let executable = resolve_executable(&agent.command, path);
            agents.push(AgentRegistration {
                is_default: default_agent_id.as_deref() == Some(agent.id.as_str()),
                id: agent.id,
                agent_type,
                display_name: agent.display_name,
                source: "configured",
                available: agent_type.is_some() && executable.is_some(),
                command: agent.command,
                executable: executable.map(|path| path.to_string_lossy().into_owned()),
                args: agent.args,
                env: agent.env,
            });
        }
        for agent_type in AgentType::ALL {
            let known = agent_type.agent().definition();
            if custom_ids.contains(known.id) {
                continue;
            }
            let executable = known
                .commands
                .iter()
                .find_map(|command| resolve_executable(command, path).map(|path| (*command, path)));
            let is_default = default_agent_id.as_deref() == Some(known.id);
            // Keep a missing default visible and unavailable until it is cleared or restored.
            if executable.is_some() || is_default {
                let (command, executable) = executable
                    .map(|(command, path)| (command, Some(path)))
                    .unwrap_or((known.commands[0], None));
                agents.push(AgentRegistration {
                    is_default,
                    id: known.id.to_owned(),
                    agent_type: Some(agent_type),
                    display_name: known.display_name.to_owned(),
                    source: "detected",
                    available: executable.is_some(),
                    command: command.to_owned(),
                    executable: executable.map(|path| path.to_string_lossy().into_owned()),
                    args: known.args.iter().map(|value| (*value).to_owned()).collect(),
                    env: BTreeMap::new(),
                });
            }
        }
        agents.sort_by_key(|agent| (type_order(agent.agent_type), agent.source != "detected"));
        Ok(agents)
    }

    pub(in crate::aow) async fn register_agent(
        &self,
        request: RegisterAgentRequest,
    ) -> Result<AgentRegistration, AowError> {
        self.save_agent(None, request).await
    }

    pub(in crate::aow) async fn save_agent(
        &self,
        existing_id: Option<&str>,
        request: RegisterAgentRequest,
    ) -> Result<AgentRegistration, AowError> {
        let _operation = self.inner.agent_operation.lock().await;
        let path = self.execution_path().await?;
        let display_name = validation::validate_display_name(&request.display_name)?;
        let command = validation::validate_command(&request.command)?;
        validation::validate_arguments(&request.args)?;
        validation::validate_env_keys(&request.env.keys().cloned().collect::<Vec<_>>())?;
        if request
            .env
            .values()
            .any(|value| value.len() > 32768 || value.contains('\0'))
        {
            return Err(AowError::Invalid(
                aow_i18n::text("validation.agent_environment_values_are_invalid").to_owned(),
            ));
        }
        let id = validation::validate_id(&request.id)?;
        let current = self.agents_in_path(&path)?;
        if let Some(existing_id) = existing_id {
            validation::validate_id(existing_id)?;
            if !current.iter().any(|agent| agent.id == existing_id) {
                return Err(AowError::AgentNotFound(existing_id.to_owned()));
            }
            if id != existing_id {
                return Err(AowError::Invalid(
                    aow_i18n::text("server.agent_id_cannot_be_changed_after_creation_delete_and")
                        .to_owned(),
                ));
            }
        }
        if existing_id.is_none() && current.iter().any(|agent| agent.id == id) {
            return Err(AowError::AgentIdConflict(id));
        }
        if AgentType::from_id(&id).is_some_and(|agent_type| agent_type != request.agent_type) {
            return Err(AowError::Invalid(
                aow_i18n::text("server.a_built_in_agent_s_type_must_match_its").to_owned(),
            ));
        }
        let stored = StoredAgent {
            id: id.clone(),
            agent_type: Some(request.agent_type),
            display_name,
            command,
            args: request.args,
            env: request.env,
        };
        {
            let mut state = self.lock()?;
            let mut agents = state.agents.clone();
            if let Some(existing) = agents
                .iter_mut()
                .find(|agent| Some(agent.id.as_str()) == existing_id)
            {
                *existing = stored;
            } else {
                agents.push(stored);
            }
            group_agents(&mut agents);
            self.persist_agents(&agents, state.default_agent_id.as_deref())?;
            state.agents = agents;
        }
        self.agents()
            .await?
            .into_iter()
            .find(|agent| agent.id == id)
            .ok_or(AowError::AgentNotFound(id))
    }

    pub(in crate::aow) async fn remove_agent(&self, id: &str) -> Result<(), AowError> {
        let _operation = self.inner.agent_operation.lock().await;
        let mut state = self.lock()?;
        let index = state
            .agents
            .iter()
            .position(|agent| agent.id == id)
            .ok_or_else(|| AowError::AgentNotFound(id.to_owned()))?;
        let mut agents = state.agents.clone();
        agents.remove(index);
        let default_agent_id = state
            .default_agent_id
            .clone()
            .filter(|default| default != id);
        self.persist_agents(&agents, default_agent_id.as_deref())?;
        state.agents = agents;
        state.default_agent_id = default_agent_id;
        Ok(())
    }
}
