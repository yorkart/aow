use super::*;
use std::collections::HashSet;

#[test]
fn task_and_run_ids_are_valid_and_unique() {
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::new(temporary.path().join("state")).unwrap();
    let mut ids = HashSet::new();
    for _ in 0..5000 {
        for id in [store.new_task_id().unwrap(), new_run_id()] {
            valid_component(&id).unwrap();
            assert!(ids.insert(id));
        }
    }
    // Allocation no longer depends on a scan for existing task files.
    assert!(!store.config_dir.join("automations/tasks").exists());
}
