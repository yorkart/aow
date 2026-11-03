//! Application-owned role bindings and prompt policy. The ACP executor receives only an environment.
use super::{AgentRole, RoleRuntime};
use crate::aow::{
    AowError, AowManager,
    registry::{RegistryDocument, atomic_save_document},
};
use aow_acp::{Content, ExecutionEnvironment, SessionCreation, SessionSnapshot, SessionSource};
use aow_acpd_client::AcpClient;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(in crate::aow) const SESSIONS_FILE: &str = "state/agent-role-sessions.json";

#[derive(Clone, Serialize, Deserialize)]
pub(in crate::aow) struct AcpRoleSession {
    pub thread_id: String,
    role_id: String,
    home: PathBuf,
    instruction: String,
}

impl AowManager {
    pub(in crate::aow) async fn acp_role_id(
        &self,
        service: &AcpClient,
        id: &str,
    ) -> anyhow::Result<Option<String>> {
        Ok(self
            .acp_role_binding(service, id)
            .await?
            .map(|binding| binding.role_id))
    }

    pub(super) fn role_acp_environment(
        &self,
        role: &AgentRole,
    ) -> Result<ExecutionEnvironment, AowError> {
        let mut env = self.role_environment(role)?;
        env.remove("HOME");
        env.remove("AOW_AGENT_ROLE_ID");
        Ok(ExecutionEnvironment {
            home: self.role_home(&role.id)?,
            env,
        })
    }

    pub(super) fn bind_role_acp_session(&self, id: &str, role: &AgentRole) -> Result<(), AowError> {
        self.save_acp_role_binding(AcpRoleSession {
            thread_id: id.into(),
            role_id: role.id.clone(),
            home: self.role_home(&role.id)?,
            instruction: role.config.instruction.clone(),
        })
    }

    fn save_acp_role_binding(&self, binding: AcpRoleSession) -> Result<(), AowError> {
        let mut sessions = self
            .inner
            .roles
            .sessions
            .lock()
            .map_err(|_| AowError::Poisoned)?;
        let mut next = sessions.clone();
        next.insert(binding.thread_id.clone(), binding);
        if let Some(directory) = &self.inner.state_dir {
            atomic_save_document(
                &directory.join(SESSIONS_FILE),
                &RegistryDocument {
                    version: 1,
                    items: next.values().cloned().collect(),
                },
            )?;
        }
        *sessions = next;
        Ok(())
    }

    async fn acp_role_binding(
        &self,
        service: &AcpClient,
        id: &str,
    ) -> anyhow::Result<Option<AcpRoleSession>> {
        let snapshot = service.snapshot(id).await?;
        let Some(environment) = &snapshot.execution_environment else {
            return Ok(None);
        };
        let sessions = self
            .inner
            .roles
            .sessions
            .lock()
            .map_err(|_| AowError::Poisoned)?;
        if let Some(binding) = sessions.get(id) {
            anyhow::ensure!(
                binding.home == environment.home,
                "Role session Home changed"
            );
            return Ok(Some(binding.clone()));
        }
        // Fork ancestry is protocol data; inheriting role instructions is an application policy.
        let parent = snapshot
            .parent
            .as_ref()
            .filter(|parent| parent.kind == aow_protocol::InstanceKind::Acp)
            .and_then(|parent| sessions.get(&parent.id))
            .filter(|binding| binding.home == environment.home)
            .cloned();
        drop(sessions);
        let binding = if let Some(mut parent) = parent {
            parent.thread_id = id.into();
            Some(parent)
        } else {
            // Sessions created/imported on an existing connection share its Home.
            // Resolve that Home to an application role here, never in the ACP crate.
            self.agent_roles()?.into_iter().find_map(|role| {
                (matches!(&role.config.runtime, RoleRuntime::Acp { agent_id } if agent_id == &snapshot.agent_id)
                    && self.role_home(&role.id).ok().as_ref() == Some(&environment.home))
                    .then(|| AcpRoleSession { thread_id: id.into(), role_id: role.id,
                        home: environment.home.clone(), instruction: role.config.instruction })
            })
        };
        if let Some(binding) = &binding {
            self.save_acp_role_binding(binding.clone())?;
        }
        Ok(binding)
    }

    pub(crate) async fn acp_role_content(
        &self,
        service: &AcpClient,
        id: &str,
        mut content: Vec<Content>,
    ) -> anyhow::Result<Vec<Content>> {
        self.collaboration_content(service, id, &mut content)
            .await?;
        if !content.is_empty()
            && let Some(binding) = self.acp_role_binding(service, id).await?
            && !binding.instruction.trim().is_empty()
        {
            content.insert(0, serde_json::from_value(serde_json::json!({
                "type":"text", "text":format!("Agent role instructions:\n{}\n\n", binding.instruction),
                "_meta":{"aow/instructions":true}
            }))?);
        }
        Ok(content)
    }

    pub(crate) async fn prepare_acp_child_session(
        &self,
        service: &AcpClient,
        agent_id: &str,
        cwd: PathBuf,
        source: SessionSource,
        parent: Option<aow_protocol::InstanceRef>,
    ) -> anyhow::Result<SessionSnapshot> {
        let inherited = if let Some(parent) = &parent
            && parent.kind == aow_protocol::InstanceKind::Acp
        {
            match self.acp_role_binding(service, &parent.id).await? {
                Some(binding) => Some((
                    binding,
                    service.snapshot(&parent.id).await?.execution_environment,
                )),
                None => None,
            }
        } else {
            None
        };
        let session = service
            .prepare_session_with_options(
                agent_id,
                cwd,
                SessionCreation {
                    source,
                    parent,
                    execution_environment: inherited
                        .as_ref()
                        .and_then(|(_, environment)| environment.clone()),
                },
            )
            .await?;
        if let Some((mut binding, _)) = inherited {
            binding.thread_id = session.thread_id.clone();
            if let Err(error) = self.save_acp_role_binding(binding) {
                let _ = service.delete_session(&session.thread_id).await;
                return Err(error.into());
            }
        }
        Ok(session)
    }
}
