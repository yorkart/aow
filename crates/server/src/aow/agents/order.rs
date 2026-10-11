use super::super::*;

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct ReorderAgentsRequest {
    pub(in crate::aow) agent_type: Option<AgentType>,
    pub(in crate::aow) ids: Vec<String>,
}

pub(super) fn type_order(agent_type: Option<AgentType>) -> usize {
    AgentType::ALL
        .iter()
        .position(|&known| Some(known) == agent_type)
        .unwrap_or(AgentType::ALL.len())
}

fn stored_type(agent: &StoredAgent) -> Option<AgentType> {
    agent.agent_type.or_else(|| AgentType::from_id(&agent.id))
}

pub(super) fn group_agents(agents: &mut [StoredAgent]) {
    agents.sort_by_key(|agent| type_order(stored_type(agent)));
}

impl AowManager {
    pub(in crate::aow) async fn reorder_agents(
        &self,
        request: ReorderAgentsRequest,
    ) -> Result<(), AowError> {
        let _operation = self.inner.agent_operation.lock().await;
        let mut state = self.lock()?;
        let group = state
            .agents
            .iter()
            .filter(|agent| stored_type(agent) == request.agent_type)
            .map(|agent| agent.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let requested = request
            .ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        if request.ids.len() != group.len() || requested != group {
            return Err(AowError::Invalid(
                "Agent 列表已变化，请刷新后仅对同类型的配置实例排序".to_owned(),
            ));
        }
        let mut agents = state.agents.clone();
        agents.sort_by_key(|agent| {
            (
                type_order(stored_type(agent)),
                request
                    .ids
                    .iter()
                    .position(|id| id == &agent.id)
                    .unwrap_or(0),
            )
        });
        self.persist_agents(&agents)?;
        state.agents = agents;
        Ok(())
    }
}
