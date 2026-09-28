use super::*;
use std::fs;

#[test]
fn task_id_retries_when_the_candidate_already_exists() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("state")).unwrap();
    private_dir(&store.config_dir.join("automations/tasks")).unwrap();
    fs::write(store.task_path("12345678").unwrap(), b"{}").unwrap();
    let mut candidates = ["12345678", "87654321"].into_iter();

    let id = store
        .new_task_id_with(|| candidates.next().unwrap().to_owned())
        .unwrap();

    assert_eq!(id, "87654321");
}

#[test]
fn run_id_has_a_timestamp_and_four_digit_random_suffix() {
    let id = new_run_id();
    let (timestamp, suffix) = id.rsplit_once('_').unwrap();
    assert_eq!(timestamp.len(), 19);
    assert_eq!(suffix.len(), 4);
    assert!(timestamp.ends_with('Z'));
    assert!(suffix.bytes().all(|byte| byte.is_ascii_digit()));
}
