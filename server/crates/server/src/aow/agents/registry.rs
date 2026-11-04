use std::collections::BTreeMap;

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
        let runtime = self.agent_node().await?;
        let (profiles, default_agent_id) = {
            let state = self.lock()?;
            (state.agents.clone(), state.default_agent_id.clone())
        };
        let request = aow_runtime_client::agents::DiscoverAgents {
            profiles: serde_json::from_value(serde_json::to_value(profiles)?)?,
            default_agent_id,
        };
        let mut agents = runtime
            .discover_agents(&request)
            .await
            .map_err(|e| AowError::Invalid(e.to_string()))?;
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
        let current = self.agents().await?;
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
