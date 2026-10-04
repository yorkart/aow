use std::{
    collections::{BTreeMap, HashSet},
    os::unix::fs::PermissionsExt,
};

use super::super::*;

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct RegisterAgentRequest {
    pub(in crate::aow) id: Option<String>,
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
    pub(in crate::aow) async fn agents(&self) -> Result<Vec<AgentRegistration>, AowError> {
        let path = self.execution_path().await?;
        self.agents_in_path(&path)
    }

    pub(in crate::aow) fn agents_in_path(
        &self,
        path: &[PathBuf],
    ) -> Result<Vec<AgentRegistration>, AowError> {
        let custom = self.lock()?.agents.clone();
        let custom_ids = custom
            .iter()
            .map(|agent| agent.id.clone())
            .collect::<HashSet<_>>();
        let mut agents = Vec::new();
        for agent in custom {
            let agent_type = agent.agent_type.or_else(|| AgentType::from_id(&agent.id));
            let executable = resolve_executable(&agent.command, path);
            agents.push(AgentRegistration {
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
            if let Some((command, executable)) = executable {
                agents.push(AgentRegistration {
                    id: known.id.to_owned(),
                    agent_type: Some(agent_type),
                    display_name: known.display_name.to_owned(),
                    source: "detected",
                    available: true,
                    command: command.to_owned(),
                    executable: Some(executable.to_string_lossy().into_owned()),
                    args: known.args.iter().map(|value| (*value).to_owned()).collect(),
                    env: BTreeMap::new(),
                });
            }
        }
        agents.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        Ok(agents)
    }

    pub(in crate::aow) async fn register_agent(
        &self,
        request: RegisterAgentRequest,
    ) -> Result<AgentRegistration, AowError> {
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
                "agent environment values are invalid".to_owned(),
            ));
        }
        let id = request
            .id
            .as_deref()
            .map(validation::validate_id)
            .transpose()?
            .unwrap_or_else(aow_id::new_id);
        if AgentType::from_id(&id).is_some_and(|agent_type| agent_type != request.agent_type) {
            return Err(AowError::Invalid(
                "内置 Agent 的类型必须与其 ID 一致；其他类型请注册为新配置".to_owned(),
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
            if let Some(existing) = agents.iter_mut().find(|agent| agent.id == id) {
                *existing = stored;
            } else {
                agents.push(stored);
            }
            self.persist_agents(&agents)?;
            state.agents = agents;
        }
        self.agents()
            .await?
            .into_iter()
            .find(|agent| agent.id == id)
            .ok_or(AowError::AgentNotFound(id))
    }

    pub(in crate::aow) fn remove_agent(&self, id: &str) -> Result<(), AowError> {
        let mut state = self.lock()?;
        let index = state
            .agents
            .iter()
            .position(|agent| agent.id == id)
            .ok_or_else(|| AowError::AgentNotFound(id.to_owned()))?;
        let removed = state.agents.remove(index);
        if let Err(error) = self.persist_agents(&state.agents) {
            state.agents.insert(index, removed);
            return Err(error);
        }
        Ok(())
    }
}
