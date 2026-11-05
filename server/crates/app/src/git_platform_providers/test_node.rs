//! External executable fixture; never evaluates provider scripts in Server.
use aow_runtime_client::{RuntimeClient, testing::TestRuntime};
use std::path::PathBuf;
pub(super) async fn start(paths: &[PathBuf]) -> (TestRuntime, RuntimeClient) {
    let directory = tempfile::tempdir().unwrap();
    let node = TestRuntime::start(&directory.path().join("runtime.json")).await;
    let client = RuntimeClient::new(node.endpoint.clone());
    client.set_execution_path(paths).await.unwrap();
    (node, client)
}
