use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

use aow_config::ConfigStore;

#[test]
fn initializes_one_home_and_preserves_existing_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home with spaces");
    let store = ConfigStore::initialize(&home).unwrap();
    let content = b"// preserved JSONC\n{\"agent_servers\":{},}\n";
    store.save(Path::new("aow-acp.json"), content).unwrap();
    let before = fs::metadata(home.join("aow-acp.json"))
        .unwrap()
        .modified()
        .unwrap();
    ConfigStore::initialize(&home)
        .unwrap()
        .save(Path::new("aow-acp.json"), content)
        .unwrap();
    assert_eq!(fs::read(home.join("aow-acp.json")).unwrap(), content);
    assert_eq!(
        fs::metadata(home.join("aow-acp.json"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert_eq!(
        fs::metadata(home.join("aow-acp.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&home).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(!home.join(".git").exists());
    assert!(!home.join("config.toml").exists());
    assert_eq!(store.directory(), home.canonicalize().unwrap());
}

#[test]
fn read_only_open_does_not_create_the_home() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("absent");
    assert_eq!(ConfigStore::open(&home).unwrap().directory(), home);
    assert!(!home.exists());
}

#[test]
fn rejects_unsupported_paths_and_symlink_writes() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path().join("home");
    let store = ConfigStore::initialize(&home).unwrap();
    let outside = temporary.path().join("outside");
    fs::create_dir(&outside).unwrap();
    for path in [
        "../aow-acp.json",
        "credentials.json",
        "/aow-acp.json",
        "automations/tasks/../../aow-acp.json",
    ] {
        assert!(store.save(Path::new(path), b"{}").is_err(), "{path}");
    }
    symlink(&outside, home.join("automations")).unwrap();
    assert!(
        store
            .save(Path::new("automations/tasks/task.json"), b"{}")
            .is_err()
    );
    symlink(outside.join("settings.json"), home.join("aow-acp.json")).unwrap();
    assert!(store.save(Path::new("aow-acp.json"), b"{}").is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn concurrent_directory_updates_are_serialized() {
    let temporary = tempfile::tempdir().unwrap();
    let store = ConfigStore::initialize(temporary.path()).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..12 {
            let store = store.clone();
            scope.spawn(move || {
                store
                    .update_directory(Path::new("data/inbox"), |directory| {
                        let path = directory.join("counter");
                        let value = if path.exists() {
                            fs::read_to_string(&path)?.parse::<u32>()?
                        } else {
                            0
                        };
                        std::thread::yield_now();
                        fs::write(path, (value + 1).to_string())?;
                        Ok(())
                    })
                    .unwrap();
            });
        }
    });
    assert_eq!(
        fs::read_to_string(temporary.path().join("data/inbox/counter")).unwrap(),
        "12"
    );
}
