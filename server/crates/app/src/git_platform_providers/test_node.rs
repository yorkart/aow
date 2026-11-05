//! External executable fixture; never evaluates provider scripts in Server.
use aow_host_client::{HostClient, testing::TestHost};
use std::path::PathBuf;
pub(super) async fn start(paths: &[PathBuf]) -> (TestHost, HostClient) {
    let directory = tempfile::tempdir().unwrap();
    let node = TestHost::start(&directory.path().join("runtime.json")).await;
    let client = HostClient::new(node.endpoint.clone());
    client.set_execution_path(paths).await.unwrap();
    (node, client)
}
