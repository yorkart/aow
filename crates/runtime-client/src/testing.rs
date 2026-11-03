//! Real TCP runtime fixtures shared by SDK and server integration tests.
use crate::RuntimeEndpoint;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

pub struct TestRuntime {
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<anyhow::Result<()>>>,
    _state: tempfile::TempDir,
    pub endpoint: RuntimeEndpoint,
}
impl TestRuntime {
    pub async fn start(connection_file: &Path) -> Self {
        Self::with_worker(connection_file, None, None).await
    }
    pub async fn with_worker(
        connection_file: &Path,
        vt_node: Option<PathBuf>,
        vt_worker: Option<PathBuf>,
    ) -> Self {
        Self::start_inner(connection_file, vt_node, vt_worker, None).await
    }
    pub async fn with_vt_config(
        connection_file: &Path,
        config: aow_terminald::VtWorkerConfig,
    ) -> Self {
        Self::start_inner(connection_file, None, None, Some(config)).await
    }
    async fn start_inner(
        connection_file: &Path,
        vt_node: Option<PathBuf>,
        vt_worker: Option<PathBuf>,
        vt_override: Option<aow_terminald::VtWorkerConfig>,
    ) -> Self {
        let state = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let token = "runtime-integration-test-token-0123456789abcdef".to_string();
        let endpoint = RuntimeEndpoint::new(&format!("http://{address}"), token.clone()).unwrap();
        std::fs::create_dir_all(connection_file.parent().unwrap()).unwrap();
        std::fs::write(
            connection_file,
            serde_json::json!({"url":endpoint.url.as_str(),"token":token}).to_string(),
        )
        .unwrap();
        std::fs::set_permissions(connection_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let (stop, stopped) = oneshot::channel();
        let options = aow_runtime_daemon::Options {
            state_dir: state.path().into(),
            listen: address,
            token,
            allowed_origins: Vec::new(),
            vt_node,
            vt_worker,
        };
        let task = tokio::spawn(aow_runtime_daemon::run_with_listener_and_vt_config(
            options,
            listener,
            async {
                let _ = stopped.await;
            },
            vt_override,
        ));
        let client = crate::RuntimeClient::new(endpoint.clone());
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if client.health().await.is_ok() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Self {
            stop: Some(stop),
            task: Some(task),
            _state: state,
            endpoint,
        }
    }
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.stop.take().unwrap().send(()).ok();
        self.task.take().unwrap().await?
    }
}
impl Drop for TestRuntime {
    fn drop(&mut self) {
        self.stop.take();
    }
}

pub async fn run(
    connection_file: PathBuf,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> anyhow::Result<()> {
    let runtime = TestRuntime::start(&connection_file).await;
    shutdown.await;
    runtime.shutdown().await
}
pub async fn run_with_worker(
    connection_file: PathBuf,
    shutdown: impl std::future::Future<Output = ()> + Send,
    config: Option<aow_terminald::VtWorkerConfig>,
) -> anyhow::Result<()> {
    let runtime = match config {
        Some(config) => TestRuntime::with_vt_config(&connection_file, config).await,
        None => TestRuntime::start(&connection_file).await,
    };
    shutdown.await;
    runtime.shutdown().await
}
