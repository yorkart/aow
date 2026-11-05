use super::{AgentRole, model::IdentityPreset};
use crate::error::ServiceError;
use crate::roles::RoleService;
pub(super) fn spec(role: &AgentRole) -> aow_host_contract::environment::RoleEnvironmentSpec {
    aow_host_contract::environment::RoleEnvironmentSpec {
        id: role.id.clone(),
        home_layout: role.config.home_layout,
        identity: role.config.identity.clone(),
        skills: role.config.skills.clone(),
    }
}
pub(super) struct PreparedRole {
    client: aow_host_client::HostClient,
    token: String,
    spec: aow_host_contract::environment::RoleEnvironmentSpec,
    published: bool,
}
impl Drop for PreparedRole {
    fn drop(&mut self) {
        if !self.published
            && let Ok(handle) = tokio::runtime::Handle::try_current()
        {
            let client = self.client.clone();
            let token = self.token.clone();
            let spec = self.spec.clone();
            handle.spawn(async move {
                if let Err(error) = client.finish_role_environment(&token, &spec, false).await {
                    tracing::error!(%error,"cannot roll back role environment");
                }
            });
        }
    }
}
impl PreparedRole {
    pub(super) async fn commit(mut self) {
        self.published = true;
        let client = self.client.clone();
        let token = self.token.clone();
        let spec = self.spec.clone();
        // A committed Server record is reconciled even if its HTTP request is cancelled.
        let finalizer = aow_i18n::spawn(async move {
            loop {
                match client.finish_role_environment(&token, &spec, true).await {
                    Ok(()) => break,
                    Err(error) => {
                        tracing::error!(%error,"cannot finalize published role environment; retrying")
                    }
                };
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        });
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), finalizer).await;
    }
    pub(super) async fn rollback(mut self) -> Result<(), ServiceError> {
        self.client
            .finish_role_environment(&self.token, &self.spec, false)
            .await
            .map_err(|error| ServiceError::Invalid(error.to_string()))?;
        self.published = true;
        Ok(())
    }
}
impl RoleService {
    pub(super) async fn resolve_node_role(&self, role: &AgentRole) -> Result<(), ServiceError> {
        let runtime = self.node.agent_node().await?;
        let _operation = self.inner.roles.operation.lock().await;
        if self.agent_role(&role.id)?.config != role.config {
            return Err(ServiceError::Invalid(
                "Role changed while preparing its environment; retry the operation".into(),
            ));
        }
        let environment = runtime
            .role_environment(&spec(role))
            .await
            .map_err(|error| ServiceError::Invalid(error.to_string()))?;
        self.inner
            .roles
            .environments
            .lock()
            .map_err(|_| ServiceError::Poisoned)?
            .insert(role.id.clone(), environment);
        Ok(())
    }
    pub(super) async fn prepare_node_role(
        &self,
        role: &AgentRole,
        previous: Option<&AgentRole>,
    ) -> Result<PreparedRole, ServiceError> {
        let runtime = self.node.agent_node().await?;
        if let Some(previous) = previous {
            runtime
                .role_environment(&spec(previous))
                .await
                .map_err(|error| ServiceError::Invalid(error.to_string()))?;
        }
        let preparation = PreparedRole {
            client: runtime,
            token: uuid::Uuid::new_v4().to_string(),
            spec: spec(role),
            published: false,
        };
        let prepared = preparation
            .client
            .prepare_role_environment(&preparation.spec, &preparation.token)
            .await
            .map_err(|error| ServiceError::Invalid(error.to_string()))?;
        self.inner
            .roles
            .environments
            .lock()
            .map_err(|_| ServiceError::Poisoned)?
            .insert(role.id.clone(), prepared.environment);
        Ok(preparation)
    }
    pub(super) async fn node_role_identity_sources(
        &self,
    ) -> Result<Vec<IdentityPreset>, ServiceError> {
        self.node
            .agent_node()
            .await?
            .role_identity_sources()
            .await
            .map_err(|error| ServiceError::Invalid(error.to_string()))
    }
}
