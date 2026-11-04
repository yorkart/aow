use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "subprocess fixture for concurrent_processes_have_distinct_nodes"]
fn allocate_in_subprocess() {
    let output = std::env::var_os("AOW_ID_TEST_OUTPUT").unwrap();
    let ids: Vec<_> = (0..5000).map(|_| aow_id::new_id()).collect();
    fs::write(output, ids.join("\n")).unwrap();
    // Keep the node leased until all siblings have generated their IDs.
    std::io::stdin().read_exact(&mut [0]).unwrap();
}

#[test]
fn concurrent_processes_have_distinct_nodes() {
    let root = tempfile::tempdir().unwrap();
    let mut children = Vec::new();
    for index in 0..4 {
        let output = root.path().join(index.to_string());
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "allocate_in_subprocess",
                "--ignored",
                "--nocapture",
            ])
            .env("AOW_ID_TEST_OUTPUT", &output)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        children.push((child, output));
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while children.iter().any(|(_, output)| !output.exists()) {
        assert!(
            Instant::now() < deadline,
            "child processes did not generate IDs"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut nodes = HashSet::new();
    let mut all_ids = HashSet::new();
    for (mut child, path) in children {
        child.stdin.take().unwrap().write_all(b"x").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let ids: Vec<_> = fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|id| id.parse::<aow_id::Snowflake>().unwrap())
            .collect();
        assert_eq!(ids.len(), 5000);
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(nodes.insert(ids[0].node_id()));
        for id in ids {
            assert!(all_ids.insert(id));
        }
    }
    assert_eq!(all_ids.len(), 20_000);
}
