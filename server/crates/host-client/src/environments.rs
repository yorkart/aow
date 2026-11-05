//! Node-owned execution environment transport. Caller business models remain independent.
use crate::{HostClient, HostError};
pub use aow_host_contract::environment::{
    EnvironmentPreparation, PreparedEnvironment as NodeEnvironment,
};
use aow_host_contract::environment::{FinishEnvironment, PrepareEnvironment, RoleEnvironmentSpec};
impl HostClient {
    pub async fn prepare_role_environment(
        &self,
        spec: &RoleEnvironmentSpec,
        token: &str,
    ) -> Result<EnvironmentPreparation, HostError> {
        self.post_json(
            "/v1/environments/roles/prepare",
            &PrepareEnvironment {
                spec: spec.clone(),
                token: Some(token.into()),
                published: false,
            },
        )
        .await
    }
    pub async fn finish_role_environment(
        &self,
        token: &str,
        spec: &RoleEnvironmentSpec,
        commit: bool,
    ) -> Result<(), HostError> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/environments/roles/finish",
                &FinishEnvironment {
                    token: token.into(),
                    spec: spec.clone(),
                    commit,
                },
            )
            .await?;
        Ok(())
    }
    pub async fn role_environment(
        &self,
        spec: &RoleEnvironmentSpec,
    ) -> Result<NodeEnvironment, HostError> {
        self.post_json(
            "/v1/environments/roles/resolve",
            &PrepareEnvironment {
                spec: spec.clone(),
                published: true,
                token: None,
            },
        )
        .await
    }
}

impl HostClient {
    pub async fn role_identity_sources(
        &self,
    ) -> Result<Vec<aow_host_contract::environment::IdentityPreset>, HostError> {
        self.post_json("/v1/environments/identity-sources", &())
            .await
    }
    pub async fn probe_role_environment(
        &self,
        request: &aow_host_contract::environment::ProbeEnvironment,
    ) -> Result<(), HostError> {
        let _: serde_json::Value = self
            .post_json("/v1/environments/roles/probe", request)
            .await?;
        Ok(())
    }
}

impl HostClient {
    pub async fn node_environment(
        &self,
    ) -> Result<aow_host_contract::environment::NodeEnvironment, HostError> {
        self.get_json("/v1/environment").await
    }
    pub async fn resolve_directory(
        &self,
        path: &std::path::Path,
    ) -> Result<std::path::PathBuf, HostError> {
        let result: aow_host_contract::workspace::PathResponse = self
            .post_json(
                "/v1/directories/resolve",
                &aow_host_contract::workspace::PathResponse { path: path.into() },
            )
            .await?;
        Ok(result.path)
    }
}
