use super::*;

#[test]
fn simultaneous_leases_use_distinct_nodes_and_reuse_waits_for_a_later_window() {
    let root = tempfile::tempdir().unwrap();
    let first = LocalGenerator::acquire(root.path()).unwrap();
    let second = LocalGenerator::acquire(root.path()).unwrap();
    assert_ne!(first.generator.node_id(), second.generator.node_id());
    let previous = first.generator.generate().unwrap();
    drop(first);
    let next = LocalGenerator::acquire(root.path()).unwrap();
    let current = next.generator.generate().unwrap();
    assert_eq!(previous.node_id(), current.node_id());
    assert!(current.timestamp_millis() > previous.timestamp_millis());
}

#[test]
fn node_directory_rejects_symlinks_and_shared_permissions() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("nodes");
    let uid = unsafe { libc::geteuid() };
    prepare_directory(&directory, uid).unwrap();
    let link = root.path().join("link");
    symlink(&directory, &link).unwrap();
    assert!(prepare_directory(&link, uid).is_err());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(prepare_directory(&directory, uid).is_err());
}
