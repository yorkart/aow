//! Node-owned execution environment transport. Caller business models remain independent.
use crate::{RuntimeClient, RuntimeError};
pub use aow_runtime_contract::{EnvironmentPreparation, NodeEnvironment};
impl RuntimeClient {
    pub async fn prepare_role_environment(
        &self,
        spec: &serde_json::Value,
        token: &str,
    ) -> Result<EnvironmentPreparation, RuntimeError> {
        self.post_json(
            "/v1/environments/roles/prepare",
            &serde_json::json!({"spec":spec,"token":token}),
        )
        .await
    }
    pub async fn finish_role_environment(
        &self,
        token: &str,
        spec: &serde_json::Value,
        commit: bool,
    ) -> Result<(), RuntimeError> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/environments/roles/finish",
                &serde_json::json!({"token":token,"spec":spec,"commit":commit}),
            )
            .await?;
        Ok(())
    }
    pub async fn role_environment(
        &self,
        spec: &serde_json::Value,
    ) -> Result<NodeEnvironment, RuntimeError> {
        self.post_json(
            "/v1/environments/roles/resolve",
            &serde_json::json!({"spec":spec,"published":true}),
        )
        .await
    }
}
