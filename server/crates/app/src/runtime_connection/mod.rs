//! Access to the configured execution node. No local execution implementation.
use crate::error::ServiceError;
use std::path::{Path, PathBuf};
use std::sync::Arc;
mod operations;
#[cfg(test)]
mod testing;
pub(crate) use operations::node_error;
#[derive(Clone)]
pub(crate) struct RuntimeConnection {
    inner: Arc<Connection>,
}
struct Connection {
    runtime_client: Option<aow_runtime_client::RuntimeClient>,
    #[cfg(test)]
    test_node_root: tempfile::TempDir,
    #[cfg(test)]
    test_runtime: std::sync::Mutex<Option<Arc<dyn Send + Sync>>>,
    #[cfg(test)]
    state_dir: Option<PathBuf>,
    #[cfg(test)]
    config: Option<aow_config::ConfigStore>,
    #[cfg(test)]
    execution_settings:
        std::sync::Mutex<Option<Arc<std::sync::Mutex<crate::settings::AowSettings>>>>,
    #[cfg(test)]
    operation: Arc<tokio::sync::Mutex<()>>,
}
impl RuntimeConnection {
    pub(crate) fn new(
        runtime_client: Option<aow_runtime_client::RuntimeClient>,
        _state_dir: Option<&Path>,
        _config: Option<aow_config::ConfigStore>,
    ) -> Self {
        Self {
            inner: Arc::new(Connection {
                runtime_client,
                #[cfg(test)]
                test_node_root: tempfile::tempdir().expect("test node root"),
                #[cfg(test)]
                test_runtime: Default::default(),
                #[cfg(test)]
                state_dir: _state_dir.map(Path::to_path_buf),
                #[cfg(test)]
                config: _config,
                #[cfg(test)]
                execution_settings: Default::default(),
                #[cfg(test)]
                operation: Arc::default(),
            }),
        }
    }
    pub(crate) async fn agent_node(
        &self,
    ) -> Result<aow_runtime_client::RuntimeClient, ServiceError> {
        if let Some(runtime) = self.node() {
            return Ok(runtime.clone());
        }
        #[cfg(test)]
        {
            self.test_acp_client()
                .map_err(ServiceError::Configuration)?
                .test_runtime()
                .await
                .map_err(ServiceError::Configuration)
        }
        #[cfg(not(test))]
        {
            Err(ServiceError::Invalid("Runtime is unavailable".into()))
        }
    }
}
