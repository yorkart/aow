use super::AcpRuntime;

#[tokio::test]
async fn closing_owner_releases_lock_even_when_an_inherited_descriptor_remains_open() {
    let root = tempfile::tempdir().unwrap();
    let first = AcpRuntime::open(root.path().into()).await.unwrap();
    // dup has the same lock lifetime as a descriptor temporarily inherited by fork.
    let inherited = first.lock.0.try_clone().unwrap();
    assert!(AcpRuntime::open(root.path().into()).await.is_err());
    first.shutdown().await.unwrap();
    drop(first);
    let second = AcpRuntime::open(root.path().into()).await.unwrap();
    drop(inherited);
    // Releasing the old description must not unlock the new owner's description.
    assert!(AcpRuntime::open(root.path().into()).await.is_err());
    second.shutdown().await.unwrap();
}
