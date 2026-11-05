use super::*;
#[test]
fn scheduling_lock_stays_exclusive_until_released() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("slot.lock");
    let first = std::fs::File::create(&path).unwrap();
    assert!(try_exclusive(&first).unwrap());
    let second = std::fs::File::open(&path).unwrap();
    assert!(!try_exclusive(&second).unwrap());
    drop(first);
    assert!(try_exclusive(&second).unwrap());
}
