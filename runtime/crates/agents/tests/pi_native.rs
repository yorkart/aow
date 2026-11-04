#![cfg(all(feature = "sessions", feature = "automation", feature = "launch"))]

use aow_agents::{
    Agent,
    automation::AgentAutomation,
    launch::{AgentLaunch, AgentType},
    sessions::{SessionEnvironment, SessionRoots, find_session, tracking::AgentSessionTracker},
};
use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
#[ignore = "requires AOW_PI_TEST_CLI; uses an isolated profile and offline provider"]
fn installed_pi_round_trips_through_aow_history_and_results() {
    let cli = std::env::var_os("AOW_PI_TEST_CLI").expect("AOW_PI_TEST_CLI");
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let environment = SessionEnvironment::from([
        ("PI_CODING_AGENT_DIR".into(), root.clone()),
        ("PI_CODING_AGENT_SESSION_DIR".into(), root.join("sessions")),
    ]);
    let roots = SessionRoots::from_configuration(&root, &environment);
    let adapter = Agent::Pi.automation().unwrap();
    let run = |args: Vec<String>| {
        let mut child = Command::new(&cli)
            .args([
                "--offline",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-context-files",
                "--model",
                "aow-test/fixture",
                "--extension",
            ])
            .arg(source.join("tests/fixtures/pi-provider.mjs"))
            .args(args)
            .envs(&environment)
            .env("PI_OFFLINE", "1")
            .env_remove("AOW_PI_BINDING")
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"Offline integration test")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(adapter
        .automation_arguments(false, Some("aow-native-session"))
        .unwrap());
    let session = find_session("pi", "aow-native-session", roots.clone()).unwrap();
    let tracker = Agent::Pi.session_tracking().unwrap();
    assert_eq!(
        tracker
            .completed_run_result(&session.locator(), chrono::Utc::now())
            .unwrap()
            .as_deref(),
        Some("Pi fixture reply")
    );
    let mut launch = AgentLaunch {
        agent_type: AgentType::Pi,
        display_name: "Pi".into(),
        executable: cli.to_string_lossy().into(),
        args: vec!["--print".into(), "--no-approve".into()],
        env: BTreeMap::new(),
    };
    launch.resume_session("aow-native-session").unwrap();
    run(launch.args);
    let snapshot = aow_agents::sessions::snapshot::read(session.locator()).unwrap();
    let snapshot = serde_json::to_value(snapshot).unwrap();
    assert_eq!(snapshot["turns"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["turns"][1]["final"]["text"], "Pi fixture reply");
    assert_eq!(snapshot["turns"][1]["usage"]["total_tokens"], 20);
}
