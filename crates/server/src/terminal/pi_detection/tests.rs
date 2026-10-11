#![cfg(any(target_os = "linux", target_os = "macos"))]

use super::*;
use crate::terminal::tests::{pane, start_daemon, stop_daemon, tab_with};
use serde_json::json;

#[tokio::test]
async fn legacy_detection_requires_a_live_pi_binding_and_preserves_osc_titles() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let socket = root.join("terminald/daemon.sock");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket);
    let mut pane = pane("managed-pi", TerminalPaneStatus::Running);
    pane.cwd = root.to_string_lossy().into_owned();
    pane.kind = TerminalPaneKind::Agent;
    pane.agent_id = Some("pi".into());
    let mut environment = Default::default();
    aow_agents::pi_bridge::prepare(&root, &pane.id, &mut pane.arguments, &mut environment).unwrap();
    let binding = aow_agents::pi_bridge::binding_path(&pane.arguments, &pane.id).unwrap();
    let pid_file = root.join("pid");
    environment.insert("PID_FILE".into(), pid_file.to_string_lossy().into_owned());
    client
        .create(
            &pane.id,
            &TerminalRuntimeSpec {
                cwd: pane.cwd.clone(),
                shell: "/bin/bash".into(),
                arguments: vec![
                    "-c".into(),
                    "exec -a pi /bin/sh -c 'echo $$ > \"$PID_FILE\"; read -r finish'".into(),
                ],
                environment,
                rows: 24,
                cols: 80,
            },
        )
        .await
        .unwrap();
    let pid: i32 = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(&pid_file)
                && let Ok(pid) = value.trim().parse()
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let tabs = [tab_with(
        TerminalLayout::Pane {
            pane_id: pane.id.clone(),
        },
        vec![pane],
    )];
    let legacy = TerminalAgentList {
        agents: [("managed-pi".into(), None)].into(),
        titles: [("managed-pi".into(), "π - demo".into())].into(),
        ..Default::default()
    };
    let mut detected = legacy.clone();
    enrich(&client, &tabs, &mut detected).await;
    assert_eq!(
        detected.agents["managed-pi"], None,
        "a saved agent type is not live evidence"
    );
    std::fs::write(&binding, json!({"pid":pid,"cwd":root}).to_string()).unwrap();
    enrich(&client, &tabs, &mut detected).await;
    assert_eq!(detected.agents["managed-pi"].as_deref(), Some("pi"));
    assert_eq!(detected.titles["managed-pi"], "π - demo");
    assert_eq!(detected.processes["managed-pi"].pid, pid);
    assert_eq!(
        Path::new(
            detected.processes["managed-pi"]
                .pi_binding
                .as_ref()
                .unwrap()
        ),
        binding
    );
    assert!(identify(-1, root.to_str().unwrap(), &binding).is_none());
    assert!(identify(std::process::id() as i32, "/another/workspace", &binding).is_none());
    detected = legacy.clone();
    detected
        .agents
        .insert("managed-pi".into(), Some("codex".into()));
    enrich(&client, &tabs, &mut detected).await;
    assert_eq!(detected.agents["managed-pi"].as_deref(), Some("codex"));
    client.delete("managed-pi").await.unwrap();
    detected = legacy;
    enrich(&client, &tabs, &mut detected).await;
    assert_eq!(detected.agents["managed-pi"], None);
    assert!(detected.processes.is_empty());
    stop_daemon(shutdown, daemon).await;
}
