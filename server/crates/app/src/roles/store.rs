use super::{AgentRole, model::RoleConfig};
use crate::error::ServiceError;
use crate::roles::RoleService;
use crate::settings::persistence::RegistryDocument;
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Default)]
pub(crate) struct RoleStore {
    pub items: Mutex<Vec<AgentRole>>,
    pub sessions: Mutex<BTreeMap<String, super::AcpRoleSession>>,
    pub environments: Mutex<BTreeMap<String, aow_runtime_client::NodeEnvironment>>,
    pub operation: tokio::sync::Mutex<()>,
}

pub(super) const FILE: &str = "aow-agent-roles.json";

impl RoleService {
    pub(crate) fn agent_roles(&self) -> Result<Vec<AgentRole>, ServiceError> {
        Ok(self
            .inner
            .roles
            .items
            .lock()
            .map_err(|_| ServiceError::Poisoned)?
            .clone())
    }

    pub(crate) fn agent_role(&self, id: &str) -> Result<AgentRole, ServiceError> {
        self.agent_roles()?
            .into_iter()
            .find(|role| role.id == id)
            .ok_or_else(|| ServiceError::Invalid("Agent role not found".into()))
    }

    fn persist_roles(&self, roles: &[AgentRole]) -> Result<(), ServiceError> {
        if let Some(config) = &self.inner.config {
            crate::settings::persistence::save_configuration(
                self.inner.config.as_ref(),
                &config.directory().join(FILE),
                &RegistryDocument {
                    version: 1,
                    items: roles.to_vec(),
                },
            )?;
        }
        Ok(())
    }

    pub(super) async fn save_role(
        &self,
        id: Option<String>,
        config: RoleConfig,
    ) -> Result<AgentRole, ServiceError> {
        self.save_role_checked(id, config, None).await
    }

    pub(super) async fn save_role_checked(
        &self,
        id: Option<String>,
        mut config: RoleConfig,
        test: Option<&crate::AppState>,
    ) -> Result<AgentRole, ServiceError> {
        let _operation = self.inner.roles.operation.lock().await;
        if test.is_some()
            && !matches!(&config.identity, super::model::Identity::LocalHome { source: Some(source) } if !source.trim().is_empty())
        {
            return Err(ServiceError::Invalid(
                "Configure a source Home before testing the connection".into(),
            ));
        }
        let creating = id.is_none() || test.is_some();
        let layout = self.role_runtime_layout(&config.runtime).await?;
        if config.home_layout != super::model::HomeLayout::Custom && config.home_layout != layout {
            return Err(ServiceError::Invalid(
                "The home layout must match the selected agent runtime".into(),
            ));
        }
        config.home_layout = layout;
        super::home::validate(&config)?;
        if test.is_some()
            && let Some(id) = &id
        {
            crate::validation::validate_id(id)?;
            if let Ok(existing) = self.agent_role(id) {
                return if existing.config == config {
                    Ok(existing)
                } else {
                    Err(ServiceError::Invalid(
                        "Role request ID belongs to another configuration".into(),
                    ))
                };
            }
        }
        let previous = if creating {
            None
        } else {
            id.as_deref().map(|id| self.agent_role(id)).transpose()?
        };
        if let Some(previous) = &previous
            && (previous.config.identity != config.identity
                || previous.config.home_layout != config.home_layout)
        {
            return Err(ServiceError::Invalid("An existing role keeps its identity and home layout; create another role to use a different identity".into()));
        }
        let role = AgentRole {
            id: id.unwrap_or_else(aow_id::new_id),
            config,
        };
        let home = self.prepare_node_role(&role, previous.as_ref()).await?;
        let result = async {
            if let Some(state) = test {
                super::connection::test(state, &role).await?;
            }
            let mut roles = self.agent_roles()?;
            if let Some(existing) = roles.iter_mut().find(|item| item.id == role.id) {
                *existing = role.clone();
            } else {
                roles.push(role.clone());
            }
            self.persist_roles(&roles)?;
            Ok::<_, ServiceError>(roles)
        }
        .await;
        let roles = match result {
            Ok(roles) => roles,
            Err(error) => {
                if let Err(rollback) = home.rollback().await {
                    tracing::error!(%rollback,"role environment rollback remains pending");
                }
                self.inner
                    .roles
                    .environments
                    .lock()
                    .map_err(|_| ServiceError::Poisoned)?
                    .remove(&role.id);
                return Err(error);
            }
        };
        home.commit().await;
        *self
            .inner
            .roles
            .items
            .lock()
            .map_err(|_| ServiceError::Poisoned)? = roles;
        Ok(role)
    }

    pub(super) async fn remove_role(&self, id: &str) -> Result<(), ServiceError> {
        let _operation = self.inner.roles.operation.lock().await;
        self.agent_role(id)?;
        let roles: Vec<_> = self
            .agent_roles()?
            .into_iter()
            .filter(|role| role.id != id)
            .collect();
        self.persist_roles(&roles)?;
        *self
            .inner
            .roles
            .items
            .lock()
            .map_err(|_| ServiceError::Poisoned)? = roles;
        // Existing sessions retain their environment. Removal never deletes their home.
        Ok(())
    }
}
