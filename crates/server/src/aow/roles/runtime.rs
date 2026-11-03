//! Resolve role references through the existing runtime configuration owners.
use super::{RoleRuntime, model::HomeLayout};
use crate::aow::{AgentLaunch, AowError, AowManager};
use anyhow::{Result, bail};
use aow_runtime_client::acp::AcpClient;
use aow_runtime_client::acp::AcpHost;
use std::{path::Path, sync::Arc};

pub(super) fn layout_for(runtime: &str) -> HomeLayout {
    let runtime = runtime.to_ascii_lowercase();
    super::identity::SUPPORTED_LAYOUTS
        .iter()
        .copied()
        .find(|layout| {
            let id = serde_json::to_value(layout).unwrap();
            runtime
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|part| Some(part) == id.as_str())
        })
        .unwrap_or_default()
}

impl AowManager {
    pub(super) async fn role_runtime_cli_launch(&self, id: &str) -> Result<AgentLaunch, AowError> {
        crate::aow::validation::validate_id(id)?;
        let paths = self.execution_path().await?;
        let agent = self
            .agents_in_path(&paths)?
            .into_iter()
            .find(|agent| agent.id == id && agent.source == "configured" && agent.available)
            .ok_or_else(|| {
                AowError::Invalid(
                    "Select an available runtime configured in Settings → Agents".into(),
                )
            })?;
        agent.into_launch(&paths).map_err(Into::into)
    }

    pub(super) fn role_acp_service(&self) -> Result<AcpClient> {
        let cache = self
            .inner
            .state_dir
            .as_ref()
            .map(|dir| dir.join("cache/acp"));
        // Shared settings and installation cache, with no persistent probe conversations.
        let host = Arc::new(RuntimeHost(self.clone()));
        match &self.inner.runtime_client {
            Some(endpoint) => AcpClient::with_runtime(endpoint.clone(), host, None, cache),
            None => AcpClient::for_test_storage(host, None, cache),
        }
    }

    pub(super) async fn role_runtime_layout(
        &self,
        runtime: &RoleRuntime,
    ) -> Result<HomeLayout, AowError> {
        let layout = match runtime {
            RoleRuntime::Cli { agent_id } => layout_for(
                self.role_runtime_cli_launch(agent_id)
                    .await?
                    .agent_type
                    .id(),
            ),
            RoleRuntime::Acp { agent_id } => {
                let service = self.role_acp_service().map_err(AowError::Configuration)?;
                if !service
                    .agents()
                    .await
                    .map_err(AowError::Configuration)?
                    .iter()
                    .any(|agent| {
                        agent.id == *agent_id
                            && agent.configured
                            && agent.installed
                            && agent.supported
                    })
                {
                    return Err(AowError::Invalid(
                        "Select an installed runtime configured in Settings → ACP".into(),
                    ));
                }
                layout_for(agent_id)
            }
        };
        super::identity::files(layout)?;
        Ok(layout)
    }
}

struct RuntimeHost(AowManager);
#[async_trait::async_trait]
impl AcpHost for RuntimeHost {
    async fn settings(&self) -> Result<String> {
        Ok(self.0.acp_settings()?.content)
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(Some(std::env::join_paths(self.0.execution_path().await?)?))
    }
    async fn read_text_file(&self, _: &Path, _: &Path) -> Result<String> {
        bail!("Connection tests do not read workspace files")
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> Result<()> {
        bail!("Connection tests do not write workspace files")
    }
}
