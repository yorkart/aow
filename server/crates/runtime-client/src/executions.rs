//! Single-execution transport. No scheduling or native agent implementation.
use crate::{RuntimeClient, RuntimeError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionReference {
    pub node_id: String,
    pub id: String,
    #[serde(default)]
    pub native_session: Option<crate::NativeSessionReference>,
}
pub use aow_runtime_contract::execution::{ExecutionEvent, ExecutionSnapshot};
use aow_runtime_contract::execution::{ExecutionSpec, SubmitExecution};
fn path(id: &str) -> Result<String, RuntimeError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid execution ID",
        )));
    }
    Ok(format!("/v1/executions/{id}"))
}
impl RuntimeClient {
    pub async fn submit_execution(
        &self,
        reference: &ExecutionReference,
        spec: ExecutionSpec,
    ) -> Result<(), RuntimeError> {
        let _: Value = self
            .put_json(
                &path(&reference.id)?,
                &SubmitExecution {
                    node_id: reference.node_id.clone(),
                    spec,
                },
            )
            .await?;
        Ok(())
    }
    pub async fn execution(
        &self,
        reference: &ExecutionReference,
        after: u64,
    ) -> Result<ExecutionSnapshot, RuntimeError> {
        self.get_json(&format!(
            "{}?after={after}&node_id={}",
            path(&reference.id)?,
            percent_encoding::utf8_percent_encode(
                &reference.node_id,
                percent_encoding::NON_ALPHANUMERIC
            )
        ))
        .await
    }
    pub async fn cancel_execution(
        &self,
        reference: &ExecutionReference,
    ) -> Result<(), RuntimeError> {
        let _: Value = self
            .post_json(
                &format!("{}/cancel", path(&reference.id)?),
                &json!({"node_id":reference.node_id}),
            )
            .await?;
        Ok(())
    }
    pub async fn execution_output(
        &self,
        reference: &ExecutionReference,
        stderr: bool,
        offset: u64,
    ) -> Result<Vec<u8>, RuntimeError> {
        #[derive(Deserialize)]
        struct Output {
            bytes: Vec<u8>,
        }
        let output: Output = self
            .get_json(&format!(
                "{}/output?output={}&offset={offset}&node_id={}",
                path(&reference.id)?,
                if stderr { "stderr" } else { "stdio" },
                percent_encoding::utf8_percent_encode(
                    &reference.node_id,
                    percent_encoding::NON_ALPHANUMERIC
                )
            ))
            .await?;
        Ok(output.bytes)
    }
    pub async fn remove_execution(
        &self,
        reference: &ExecutionReference,
    ) -> Result<(), RuntimeError> {
        self.delete(&format!(
            "{}?node_id={}",
            path(&reference.id)?,
            percent_encoding::utf8_percent_encode(
                &reference.node_id,
                percent_encoding::NON_ALPHANUMERIC
            )
        ))
        .await
    }
}
