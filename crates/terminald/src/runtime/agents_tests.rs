use super::*;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
#[ignore = "requires AOW_HERMES_TEST_PID for an existing foreground Hermes CLI; read-only"]
fn native_scan_matches_an_existing_hermes_process() {
    let pid: i32 = std::env::var("AOW_HERMES_TEST_PID")
        .unwrap()
        .parse()
        .unwrap();
    let info = aow_process::info(pid).unwrap();
    let sessions = BTreeMap::from([("live-hermes".into(), Some(info.session))]);
    let detected = scan(&sessions).unwrap();
    assert_eq!(detected.agents["live-hermes"].as_deref(), Some("hermes"));
    let process = &detected.processes["live-hermes"];
    assert_eq!(process.pid, pid);
    assert_eq!(process.start_time, info.start_time);
    assert_eq!(Path::new(&process.cwd), aow_process::cwd(pid).unwrap());
    eprintln!(
        "Hermes PID {pid}, PTY session {}, cwd {}",
        info.session, process.cwd
    );
}

fn select_agent(session: i32, processes: &BTreeMap<i32, Process>) -> Option<&'static str> {
    select_process(session, processes).and_then(|(process, _)| process.agent)
}

fn process(
    pid: i32,
    parent: i32,
    group: i32,
    session: i32,
    tty: i32,
    agent: Option<&'static str>,
) -> Process {
    Process {
        info: aow_process::ProcessInfo {
            pid,
            parent,
            group,
            session,
            tty,
            foreground: 30,
            state: 'S',
            start_time: "123".to_owned(),
        },
        agent,
    }
}

#[test]
fn detection_follows_foreground_tools_and_isolates_terminal_sessions() {
    let mut processes: BTreeMap<_, _> = [
        process(10, 1, 10, 10, 1, None),
        process(20, 10, 20, 10, 1, Some("claude")),
        process(21, 10, 21, 10, 1, Some("codex")),
        process(30, 21, 30, 10, 1, None),
        process(40, 10, 40, 10, 2, Some("traecli")),
        process(50, 1, 50, 50, 3, Some("traecli")),
    ]
    .into_iter()
    .map(|process| (process.info.pid, process))
    .collect();
    assert_eq!(select_agent(10, &processes), Some("codex"));
    assert_eq!(
        select_process(10, &processes).map(|(p, foreground)| (p.info.pid, foreground)),
        Some((21, true))
    );
    assert_eq!(select_agent(50, &processes), Some("traecli"));
    processes.remove(&21);
    assert_eq!(select_agent(10, &processes), Some("claude"));
    assert_eq!(
        select_process(10, &processes).map(|(p, foreground)| (p.info.pid, foreground)),
        Some((20, false))
    );
    processes.get_mut(&20).unwrap().info.state = 'Z';
    assert_eq!(select_agent(10, &processes), None);
    processes.get_mut(&20).unwrap().info.state = 'T';
    assert_eq!(select_agent(10, &processes), None);
    processes.get_mut(&20).unwrap().info.state = 'S';
    processes.get_mut(&20).unwrap().info.parent = 20;
    processes.get_mut(&10).unwrap().info.foreground = 20;
    assert_eq!(select_agent(10, &processes), Some("claude"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn native_scan_recognizes_all_agents_and_tracks_the_live_cwd_and_exit() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let cwd = root.join("project with spaces");
    std::fs::create_dir(&cwd).unwrap();
    for agent in ["claude", "codex", "traecli", "hermes"] {
        let ready = root.join(format!("{agent}-ready"));
        let tracker = Arc::new(SpawnTracker::default());
        // Give a harmless shell the Agent's argv[0]. This exercises native
        // argv parsing and the real matcher without invoking an AI service.
        let mut spawned = spawn_runtime_blocking(
            format!("agent-scan-{agent}"),
            TerminalRuntimeSpec {
                cwd: root.to_string_lossy().into_owned(),
                shell: "/bin/bash".into(),
                arguments: vec!["-c".into(), format!(
                    "cd \"$AOW_TEST_CWD\"; exec -a {agent} /bin/sh -c 'printf ready > \"$AOW_TEST_READY\"; read -r finish'"
                )],
                environment: BTreeMap::from([
                    ("AOW_TEST_READY".into(), ready.to_string_lossy().into_owned()),
                    ("AOW_TEST_CWD".into(), cwd.to_string_lossy().into_owned()),
                ]),
                rows: 24,
                cols: 80,
            },
            tracker.begin(),
            None,
        ).unwrap();
        let pid = spawned
            .runtime
            .child_session_id
            .expect("PTY child is a session leader");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(Instant::now() < deadline, "fixture shell did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let sessions = BTreeMap::from([("pane".into(), Some(pid)), ("exited".into(), None)]);
        let detected = scan(&sessions).unwrap();
        assert_eq!(detected.agents["pane"].as_deref(), Some(agent));
        assert_eq!(detected.agents["exited"], None);
        let selected = &detected.processes["pane"];
        assert_eq!(selected.pid, pid);
        assert_eq!(Path::new(&selected.cwd), cwd);
        assert!(aow_process::same_process(pid, &selected.start_time));

        spawned.child.as_mut().unwrap().kill().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while spawned
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none()
        {
            assert!(Instant::now() < deadline, "fixture process did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        spawned.child.take();
        let detected = scan(&sessions).unwrap();
        assert_eq!(detected.agents["pane"], None);
        assert!(!detected.processes.contains_key("pane"));
    }
}
