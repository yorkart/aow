use aow_host_daemon::{Config, load_token};
use std::os::unix::fs::PermissionsExt;

#[test]
fn token_is_private_persistent_and_rejects_insecure_or_symlink_files() {
    let root = tempfile::tempdir().unwrap();
    let first = load_token(root.path(), None).unwrap();
    assert_eq!(first.len(), 64);
    assert_eq!(load_token(root.path(), None).unwrap(), first);
    let path = root.path().join("runtime/host-daemon/token");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(load_token(root.path(), None).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(load_token(root.path(), Some(&link)).is_err());
    std::fs::write(&path, "short").unwrap();
    assert!(load_token(root.path(), None).is_err());
    assert!(load_token(root.path(), Some(&root.path().join("missing"))).is_err());
}

#[test]
fn configuration_is_strict_and_relative_resource_paths_follow_the_config_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("node.json");
    std::fs::write(&path,r#"{"listen":"127.0.0.1:0","token_file":"token","vt_worker":"worker.mjs","allowed_origins":["https://example.com"]}"#).unwrap();
    let config = Config::load(&path).unwrap();
    assert_eq!(config.listen.port(), 0);
    assert_eq!(config.token_file.unwrap(), root.path().join("token"));
    assert_eq!(config.vt_worker.unwrap(), root.path().join("worker.mjs"));
    std::fs::write(&path, r#"{"lissten":"0.0.0.0:9760"}"#).unwrap();
    assert!(Config::load(&path).is_err());
}
