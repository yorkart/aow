use super::*;

#[test]
fn partial_write_and_sync_failures_prevent_further_appends() {
    for partial in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events.jsonl");
        let mut writer = RunWriter {
            file: File::create(&path).unwrap(),
            directory: directory.path().into(),
            active_path: None,
            failure: None,
        };
        writer
            .append(&serde_json::json!({"type": "started"}))
            .unwrap();
        let error = writer
            .append_with(&serde_json::json!({"type": "workspace"}), |file, line| {
                // Inject ENOSPC after a short write, or after the full write when syncing.
                file.write_all(if partial { &line[..8] } else { line })?;
                Err(std::io::Error::from_raw_os_error(libc::ENOSPC))
            })
            .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .raw_os_error(),
            Some(libc::ENOSPC)
        );
        let before = fs::read(&path).unwrap();
        let subsequent = writer
            .append(&serde_json::json!({"type": "finished"}))
            .unwrap_err();
        assert!(
            subsequent
                .to_string()
                .contains(&error.root_cause().to_string())
        );
        assert_eq!(fs::read(path).unwrap(), before);
        assert_eq!(before.ends_with(b"\n"), !partial);
    }
}
