use super::{AgentRole, model::IdentityPreset};
use crate::aow::{AowError, AowManager};
pub(super) fn spec(role: &AgentRole) -> serde_json::Value {
    serde_json::json!({"id":role.id,"home_layout":role.config.home_layout,"identity":role.config.identity,"skills":role.config.skills})
}
pub(super) enum PreparedRole {
    Node(NodePreparation),
    #[cfg(test)]
    Local(super::home_fixture::PreparedHome),
}
pub(super) struct NodePreparation {
    client: aow_runtime_client::RuntimeClient,
    token: String,
    spec: serde_json::Value,
    published: bool,
}
impl Drop for NodePreparation {
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
    pub(super) async fn commit(self) {
        match self {
            Self::Node(mut preparation) => {
                preparation.published = true;
                let client = preparation.client.clone();
                let token = preparation.token.clone();
                let spec = preparation.spec.clone();
                // Spawn before awaiting so HTTP cancellation cannot undo a published business decision.
                let finalizer = aow_i18n::spawn(async move {
                    loop {
                        match client.finish_role_environment(&token, &spec, true).await {
                            Ok(()) => break,
                            Err(error) => {
                                tracing::error!(%error,"cannot finalize published role environment; retrying")
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                });
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), finalizer).await;
            }
            #[cfg(test)]
            Self::Local(home) => home.commit(),
        }
    }
}
impl AowManager {
    pub(super) async fn resolve_node_role(&self, role: &AgentRole) -> Result<(), AowError> {
        if let Some(runtime) = &self.inner.runtime_client {
            let _operation = self.inner.roles.operation.lock().await;
            if self.agent_role(&role.id)?.config != role.config {
                return Err(AowError::Invalid(
                    "Role changed while preparing its environment; retry the operation".into(),
                ));
            }
            let environment = runtime
                .role_environment(&spec(role))
                .await
                .map_err(|error| AowError::Invalid(error.to_string()))?;
            self.inner
                .roles
                .environments
                .lock()
                .map_err(|_| AowError::Poisoned)?
                .insert(role.id.clone(), environment);
        }
        Ok(())
    }
    pub(super) async fn prepare_node_role(
        &self,
        role: &AgentRole,
        previous: Option<&AgentRole>,
    ) -> Result<PreparedRole, AowError> {
        if let Some(runtime) = &self.inner.runtime_client {
            // Reconcile a preceding publication/rollback before admitting a new edit.
            if let Some(previous) = previous {
                runtime
                    .role_environment(&spec(previous))
                    .await
                    .map_err(|error| AowError::Invalid(error.to_string()))?;
            }
            let preparation = NodePreparation {
                client: runtime.clone(),
                token: uuid::Uuid::new_v4().to_string(),
                spec: spec(role),
                published: false,
            };
            let prepared = runtime
                .prepare_role_environment(&preparation.spec, &preparation.token)
                .await
                .map_err(|error| AowError::Invalid(error.to_string()))?;
            self.inner
                .roles
                .environments
                .lock()
                .map_err(|_| AowError::Poisoned)?
                .insert(role.id.clone(), prepared.environment);
            return Ok(PreparedRole::Node(preparation));
        }
        #[cfg(test)]
        {
            let manager = self.clone();
            let role = role.clone();
            let previous = previous.cloned();
            aow_i18n::spawn_blocking(move || {
                manager.local_prepare_role_home(&role, previous.as_ref())
            })
            .await
            .map_err(|e| AowError::Invalid(e.to_string()))?
            .map(PreparedRole::Local)
        }
        #[cfg(not(test))]
        {
            let _ = previous;
            Err(AowError::Invalid("Runtime is unavailable".into()))
        }
    }
    pub(super) async fn node_role_identity_sources(&self) -> Result<Vec<IdentityPreset>, AowError> {
        if let Some(runtime) = &self.inner.runtime_client {
            return runtime
                .post_json("/v1/environments/identity-sources", &serde_json::json!({}))
                .await
                .map_err(|e| AowError::Invalid(e.to_string()));
        }
        #[cfg(test)]
        {
            Ok(self.role_identity_sources())
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Runtime is unavailable".into()))
        }
    }
}
