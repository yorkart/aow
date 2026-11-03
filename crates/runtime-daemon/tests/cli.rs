use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, time::Duration};

#[tokio::test]
async fn binary_accepts_separate_config_and_state_root_and_stops_on_sigterm() {
    let root = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let token = "node-cli-test-token-0123456789abcdef0123456789abcdef";
    std::fs::write(config_dir.path().join("token"), token).unwrap();
    std::fs::set_permissions(
        config_dir.path().join("token"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = config_dir.path().join("node.json");
    std::fs::write(
        &config,
        json!({"listen":address.to_string(),"token_file":"token","vt_node":"/missing-node"})
            .to_string(),
    )
    .unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_aow-runtime-daemon"))
        .args([
            "--state-dir",
            root.path().to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();
    let health = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "daemon exited during startup"
            );
            if let Ok(response) = client
                .get(format!("http://{address}/v1/health"))
                .bearer_auth(token)
                .send()
                .await
                && response.status().is_success()
            {
                break response.json::<Value>().await.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(health["pid"], child.id().unwrap());
    let response=client.put(format!("http://{address}/v1/terminal/runtimes/cli"))
        .bearer_auth(token).json(&json!({"cwd":root.path(),"shell":"/bin/sh","arguments":["-c","printf '%s' \"$AOW_STATE_DIR\" > \"$AOW_ROOT_FILE\"; printf '%s' \"$$\" > \"$AOW_PID_FILE\"; exec /bin/cat"],
            "environment":{"AOW_PID_FILE":root.path().join("child.pid"),"AOW_ROOT_FILE":root.path().join("root.txt")},"rows":24,"cols":80})).send().await.unwrap();
    assert!(response.status().is_success());
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(root.path().join("child.pid"))
                && let Ok(pid) = text.parse::<i32>()
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        unsafe { libc::kill(child.id().unwrap() as i32, libc::SIGTERM) },
        0
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("root.txt")).unwrap(),
        std::fs::canonicalize(root.path())
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "terminal child survived SIGTERM"
    );
    assert!(!root.path().join("runtime/acpd/acpd.sock").exists());
    assert!(
        !root
            .path()
            .join("runtime/terminald/terminald.sock")
            .exists()
    );
}
