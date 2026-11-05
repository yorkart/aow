use crate::{EnvironmentManager, HomeLayout, Identity, PreparedHome, RoleEnvironmentSpec};
use std::os::unix::fs::{PermissionsExt, symlink};
fn role(id: &str) -> RoleEnvironmentSpec {
    RoleEnvironmentSpec {
        id: id.into(),
        home_layout: HomeLayout::Codex,
        identity: Identity::LocalHome { source: None },
        skills: vec![],
    }
}
#[test]
fn imported_identity_is_a_private_copy_and_rollback_is_replayable() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    std::fs::create_dir_all(source.join(".codex")).unwrap();
    std::fs::write(source.join(".codex/auth.json"), "original credentials").unwrap();
    let manager = EnvironmentManager::new(root.path().join("node")).unwrap();
    let mut spec = role("role-one");
    spec.identity = Identity::LocalHome {
        source: Some(source.to_string_lossy().into_owned()),
    };
    manager.prepare_role_home(&spec, None).unwrap().commit();
    let home = manager.role_home(&spec.id).unwrap();
    std::fs::write(source.join(".codex/auth.json"), "rotated credentials").unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join(".codex/auth.json")).unwrap(),
        "original credentials"
    );
    assert_eq!(
        std::fs::metadata(home.join(".codex/auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let skill = root.path().join("review");
    std::fs::create_dir(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "review").unwrap();
    let mut next = spec.clone();
    next.skills.push(skill.to_string_lossy().into_owned());
    manager
        .prepare_role_home(&next, Some(&spec))
        .unwrap()
        .commit();
    let mut removed = next.clone();
    removed.skills.clear();
    let prepared = manager.prepare_role_home(&removed, Some(&next)).unwrap();
    let journal = prepared.journal().unwrap();
    prepared.suspend();
    PreparedHome::restore(journal.clone())
        .unwrap()
        .rollback()
        .unwrap();
    PreparedHome::restore(journal).unwrap().rollback().unwrap();
    assert_eq!(
        std::fs::read_to_string(home.join(".codex/skills/review/SKILL.md")).unwrap(),
        "review"
    );
    let environment = manager.role_environment(&spec).unwrap();
    assert_eq!(environment["HOME"], home.to_str().unwrap());
    assert_eq!(
        environment["AOW_STATE_DIR"],
        root.path().join("node").to_str().unwrap()
    );
}
#[test]
fn escaping_skill_symlinks_roll_back_the_entire_new_home() {
    let root = tempfile::tempdir().unwrap();
    let manager = EnvironmentManager::new(root.path().join("node")).unwrap();
    let skill = root.path().join("review");
    std::fs::create_dir(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "review").unwrap();
    std::fs::write(root.path().join("outside"), "private").unwrap();
    symlink(root.path().join("outside"), skill.join("escape")).unwrap();
    let mut spec = role("role-two");
    spec.skills.push(skill.to_string_lossy().into_owned());
    assert!(manager.prepare_role_home(&spec, None).is_err());
    assert!(!manager.role_home(&spec.id).unwrap().exists());
    assert!(manager.role_home("../invalid").is_err());
}
