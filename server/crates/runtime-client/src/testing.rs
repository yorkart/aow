//! Black-box TCP runtime fixtures. Build runtime-test-host in the runtime workspace first.
use crate::RuntimeEndpoint;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};

/// Test-only transport configuration, independently defined by the caller.
#[derive(Clone, Debug, serde::Serialize)]
pub struct VtWorkerConfig {
    pub program: PathBuf,
    pub script: PathBuf,
    pub queue_capacity: usize,
    pub request_timeout: Duration,
    pub snapshot_interval: Duration,
    pub scrollback: usize,
}
impl VtWorkerConfig {
    pub fn new(program: impl Into<PathBuf>, script: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            script: script.into(),
            queue_capacity: 2048,
            request_timeout: Duration::from_secs(10),
            snapshot_interval: Duration::from_millis(200),
            scrollback: 5000,
        }
    }
}
pub struct TestRuntime {
    child: Option<Child>,
    _state: tempfile::TempDir,
    pub endpoint: RuntimeEndpoint,
}
impl TestRuntime {
    pub async fn start(connection_file: &Path) -> Self {
        Self::with_worker(connection_file, None, None).await
    }
    pub async fn with_worker(
        connection_file: &Path,
        node: Option<PathBuf>,
        worker: Option<PathBuf>,
    ) -> Self {
        Self::start_inner(connection_file, node, worker, None, None).await
    }
    pub async fn with_vt_config(connection_file: &Path, config: VtWorkerConfig) -> Self {
        Self::start_inner(connection_file, None, None, Some(config), None).await
    }
    pub async fn with_state_dir(connection_file: &Path, state_dir: PathBuf) -> Self {
        Self::start_inner(connection_file, None, None, None, Some(state_dir)).await
    }
    async fn start_inner(
        connection_file: &Path,
        vt_node: Option<PathBuf>,
        vt_worker: Option<PathBuf>,
        vt_override: Option<VtWorkerConfig>,
        state_dir: Option<PathBuf>,
    ) -> Self {
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(connection_file.parent().unwrap()).unwrap();
        // A restarted fixture must not observe the preceding process's credentials.
        match std::fs::remove_file(connection_file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("{e}"),
        }
        let configuration = state.path().join("host-options.json");
        std::fs::write(
            &configuration,
            serde_json::json!({
                "state_dir":state_dir.as_deref().unwrap_or(state.path()), "connection_file":connection_file,
                "token":"runtime-integration-test-token-0123456789abcdef",
                "vt_node":vt_node,"vt_worker":vt_worker,"vt_override":vt_override,
            })
            .to_string(),
        )
        .unwrap();
        std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o600)).unwrap();
        let executable = std::env::var_os("AOW_RUNTIME_TEST_HOST")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../target/debug/examples/runtime-test-host")
            });
        let mut child = Command::new(&executable)
            .arg(configuration)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            // EOF on stdin owns graceful shutdown even if the caller Tokio runtime exits.
            .kill_on_drop(false)
            .spawn()
            .unwrap_or_else(|e| {
                panic!(
                    "Start {}: {e}; build the runtime-test-host example first",
                    executable.display()
                )
            });
        let endpoint = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "runtime fixture exited before ready"
                );
                if let Ok(endpoint) = RuntimeEndpoint::from_file(connection_file)
                    && crate::RuntimeClient::new(endpoint.clone())
                        .health()
                        .await
                        .is_ok()
                {
                    break endpoint;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime fixture startup timed out");
        Self {
            child: Some(child),
            _state: state,
            endpoint,
        }
    }
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        let mut child = self.child.take().unwrap();
        drop(child.stdin.take());
        anyhow::ensure!(
            tokio::time::timeout(Duration::from_secs(15), child.wait())
                .await??
                .success(),
            "runtime fixture shutdown failed"
        );
        Ok(())
    }
}
impl Drop for TestRuntime {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(15), child.wait()).await;
                });
            }
        }
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
    config: Option<VtWorkerConfig>,
) -> anyhow::Result<()> {
    let runtime = match config {
        Some(config) => TestRuntime::with_vt_config(&connection_file, config).await,
        None => TestRuntime::start(&connection_file).await,
    };
    shutdown.await;
    runtime.shutdown().await
}
