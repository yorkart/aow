//! Node-owned execution environment transport. Caller business models remain independent.
use crate::{RuntimeClient, RuntimeError};
pub use aow_runtime_contract::environment::{
    EnvironmentPreparation, PreparedEnvironment as NodeEnvironment,
};
use aow_runtime_contract::environment::{
    FinishEnvironment, PrepareEnvironment, RoleEnvironmentSpec,
};
impl RuntimeClient {
    pub async fn prepare_role_environment(
        &self,
        spec: &RoleEnvironmentSpec,
        token: &str,
    ) -> Result<EnvironmentPreparation, RuntimeError> {
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
    ) -> Result<(), RuntimeError> {
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
    ) -> Result<NodeEnvironment, RuntimeError> {
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

impl RuntimeClient {
    pub async fn role_identity_sources(
        &self,
    ) -> Result<Vec<aow_runtime_contract::environment::IdentityPreset>, RuntimeError> {
        self.post_json("/v1/environments/identity-sources", &())
            .await
    }
    pub async fn probe_role_environment(
        &self,
        request: &aow_runtime_contract::environment::ProbeEnvironment,
    ) -> Result<(), RuntimeError> {
        let _: serde_json::Value = self
            .post_json("/v1/environments/roles/probe", request)
            .await?;
        Ok(())
    }
}

impl RuntimeClient {
    pub async fn node_environment(
        &self,
    ) -> Result<aow_runtime_contract::environment::NodeEnvironment, RuntimeError> {
        self.get_json("/v1/environment").await
    }
    pub async fn resolve_directory(
        &self,
        path: &std::path::Path,
    ) -> Result<std::path::PathBuf, RuntimeError> {
        let result: aow_runtime_contract::workspace::PathResponse = self
            .post_json(
                "/v1/directories/resolve",
                &aow_runtime_contract::workspace::PathResponse { path: path.into() },
            )
            .await?;
        Ok(result.path)
    }
}
