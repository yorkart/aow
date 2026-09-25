use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const FAKE_AGENT: &str = r#"
import os, sys, tty, time, json
tty.setraw(0)
log, mode = sys.argv[1:]
def show(text):
    os.write(1, ('\x1b[2J\x1b[H' + text + '\x1b[47;1H> \x1b[48;1Hfixture-model · /workspace · ready').encode())
os.write(1, b'\x1b[2J\x1b[Hmodel: loading\r\n> Ask anything')
time.sleep(1)
if mode == 'exit': sys.exit(9)
if mode != 'loading': show('model: fixture-model\r\ndirectory: /workspace\r\n')
buf = b''
while True:
    ch = os.read(0, 1)
    if not ch: break
    with open(log + '.raw', 'ab') as file: file.write(ch)
    if ch != b'\r':
        buf += ch
        continue
    text = buf.decode().replace('\x1b[200~', '').replace('\x1b[201~', '')
    buf = b''
    with open(log, 'a') as file: file.write(json.dumps(text) + '\n')
    show('Accepted task\r\n')
"#;

struct Fixture {
    directory: tempfile::TempDir,
    repo: PathBuf,
    project_id: String,
    log: PathBuf,
    state: AppState,
    client: TerminaldClient,
    daemon: tokio::task::JoinHandle<Result<(), aow_terminald::TerminaldError>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    web: tokio::task::JoinHandle<()>,
    address: std::net::SocketAddr,
    cli: Option<crate::local_cli::LocalCliServer>,
}

impl Fixture {
    async fn new(mode: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("terminald/daemon.sock");
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        let daemon_socket = socket.clone();
        let worker =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/dist/vt-worker.mjs");
        let daemon = tokio::spawn(async move {
            aow_terminald::run_with_shutdown_and_vt_worker(
                daemon_socket,
                async {
                    let _ = receiver.await;
                },
                Some(aow_terminald::VtWorkerConfig::new("node", worker)),
            )
            .await
        });
        let native = TerminaldClient::new(socket.clone());
        tokio::time::timeout(Duration::from_secs(5), async {
            while native.health().await.is_err() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let repo = directory.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        ] {
            let output = tokio::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut state = AppState::with_terminald_socket(PathBuf::new(), socket);
        state.operations = crate::operations::OperationService::persistent(
            &directory.path().join("operation-logs"),
        )
        .unwrap();
        let script = directory.path().join("agent.py");
        std::fs::write(&script, FAKE_AGENT).unwrap();
        let log = directory.path().join("input.jsonl");
        let app = crate::build_router(state.clone());
        let project = post(&app, "/api/aow/projects", json!({"path":repo})).await;
        let project_id = project["id"].as_str().unwrap().to_owned();
        for agent in ["codex", "traecli"] {
            post(&app, "/api/aow/agents", json!({"id":agent,"agent_type":agent,"display_name":agent,"command":"/usr/bin/python3","args":[script,log,mode]})).await;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let web = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let cli = crate::start_local_cli(state.clone(), directory.path())
            .await
            .unwrap();
        let client = TerminaldClient::new(directory.path().join("cli/cli.sock"));
        Self {
            directory,
            repo,
            project_id,
            log,
            state,
            client,
            daemon,
            shutdown: Some(shutdown),
            web,
            address,
            cli: Some(cli),
        }
    }

    fn request(&self) -> AgentTerminalCreate {
        AgentTerminalCreate {
            agent: "codex".into(),
            project_id: self.project_id.clone(),
            cwd: self.repo.to_string_lossy().into_owned(),
            task: None,
            timeout_seconds: 8,
        }
    }

    async fn browser(
        &self,
        tab: &str,
        pane: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        tokio_tungstenite::connect_async(format!("ws://{}/api/terminals/{tab}/panes/{pane}/ws?control=v2&observer=v1&capabilities=vt-snapshot-v1", self.address)).await.unwrap().0
    }

    async fn stop(mut self) {
        self.web.abort();
        self.cli.take().unwrap().shutdown().await;
        let _ = self.shutdown.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(10), &mut self.daemon)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.web.abort();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

async fn post(app: &Router, path: &str, body: Value) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(
        status.is_success(),
        "{status}: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn session_resume_launches_selected_profiles_without_changing_registration() {
    let fixture = Fixture::new("ready").await;
    let app = crate::build_router(fixture.state.clone());
    let session_cwd = fixture.repo.join("session subdirectory");
    std::fs::create_dir(&session_cwd).unwrap();
    let script = fixture.directory.path().join("resume.py");
    std::fs::write(
        &script,
        r#"import json, os, sys, time
with open(sys.argv[1], 'w') as log:
    json.dump({'args': sys.argv[2:], 'cwd': os.getcwd(), 'profile': os.environ['RESUME_PROFILE']}, log)
time.sleep(30)
"#,
    )
    .unwrap();
    for (index, agent_type) in ["codex", "codex", "claude", "traecli", "hermes"]
        .iter()
        .enumerate()
    {
        let id = format!("resume-profile-{index}");
        let log = fixture
            .directory
            .path()
            .join(format!("resume-{index}.json"));
        let args = json!([script, log, "--profile", format!("profile {index}")]);
        let env = json!({"RESUME_PROFILE": id});
        post(
            &app,
            "/api/aow/agents",
            json!({"id": id, "agent_type": agent_type, "display_name": id,
                "command": "/usr/bin/python3", "args": args, "env": env}),
        )
        .await;
        let session_id = "a49342f2-e4c3-4bab-b333-6d7d6c961a29";
        // There is deliberately no session file: the chosen CLI owns recovery.
        let tab = post(
            &app,
            "/api/terminals",
            json!({"workspace_root": fixture.repo, "cwd": session_cwd,
                "agent_id": id, "resume_session_id": session_id}),
        )
        .await;
        let resume_arg = if matches!(*agent_type, "claude" | "hermes") {
            "--resume"
        } else {
            "resume"
        };
        let mut expected_args = args.as_array().unwrap().clone();
        expected_args.extend([json!(resume_arg), json!(session_id)]);
        assert_eq!(tab["panes"][0]["arguments"], json!(expected_args));
        assert_eq!(tab["panes"][0]["kind"], "agent");
        assert_eq!(tab["panes"][0]["agent_id"], *agent_type);
        assert_eq!(tab["panes"][0]["agent_profile_id"], id);
        let invocation: Value = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(bytes) = std::fs::read(&log) {
                    if let Ok(value) = serde_json::from_slice(&bytes) {
                        break value;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            invocation,
            json!({"args": ["--profile", format!("profile {index}"), resume_arg, session_id],
            "cwd": session_cwd.canonicalize().unwrap(), "profile": id})
        );
        let launch = fixture
            .state
            .aow
            .resolve_agent_launch(&id, fixture.repo.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(json!(launch.args), args);
        assert_eq!(launch.env.get("RESUME_PROFILE"), Some(&id));
        let tab: TerminalTab = serde_json::from_value(tab).unwrap();
        let manager = &fixture.state.terminals;
        manager
            .inner
            .terminald
            .delete(&tab.panes[0].id)
            .await
            .unwrap();
        if index == 1 {
            // Legacy panes can recover an unambiguous custom profile even when
            // the saved command contains extra native-session resume arguments.
            let mut state = manager.lock_state().unwrap();
            state
                .tabs
                .iter_mut()
                .find(|item| item.id == tab.id)
                .unwrap()
                .panes[0]
                .agent_profile_id = None;
        }
        std::fs::remove_file(&log).unwrap();
        let rebuilt = post(
            &app,
            &format!("/api/terminals/{}/rebuild", tab.id),
            json!({}),
        )
        .await;
        assert_eq!(rebuilt["id"], tab.id);
        assert_ne!(rebuilt["panes"][0]["id"], tab.panes[0].id);
        assert_eq!(rebuilt["panes"][0]["arguments"], json!(expected_args));
        let replay: Value = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(bytes) = std::fs::read(&log) {
                    if let Ok(value) = serde_json::from_slice(&bytes) {
                        break value;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            replay, invocation,
            "rebuild must use the original profile environment and resume arguments"
        );
    }
    let count = fixture
        .state
        .terminals
        .list_snapshot(None)
        .unwrap()
        .tabs
        .len();
    for (agent, session_id) in [
        (None, "some-session"),
        (Some("resume-profile-0"), ""),
        (Some("resume-profile-0"), "--last"),
        (Some("resume-profile-0"), "bad\nsession"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/terminals")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"workspace_root": fixture.repo, "cwd": fixture.repo,
                "agent_id": agent, "resume_session_id": session_id})
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        fixture
            .state
            .terminals
            .list_snapshot(None)
            .unwrap()
            .tabs
            .len(),
        count
    );
    fixture.stop().await;
}

async fn control(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    force: bool,
) -> TerminalControlState {
    socket
        .send(tungstenite::Message::Text(
            json!({"type":"claim","force":force}).to_string().into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(tungstenite::Message::Text(text))) = socket.next().await {
                if let Ok(TerminalAttachServerMessage::Control { state }) =
                    serde_json::from_str(&text)
                {
                    return state;
                }
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn hidden_agent_readiness_observation_takeover_and_repeated_submission() {
    let fixture = Fixture::new("ready").await;
    let client = fixture.client.clone();
    let mut request = fixture.request();
    request.task = Some("first task\n保持多行 $HOME `literal`".into());
    let first_task = request.task.clone().unwrap();
    let creation = tokio::spawn(async move {
        client
            .post_json::<_, AgentTerminalInfo>("/v1/agents", &request)
            .await
            .unwrap()
    });
    let initial = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let tabs = fixture.state.terminals.list_snapshot(None).unwrap();
            if let Some(tab) = tabs.tabs.first() {
                break tab.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let pane = &initial.panes[0];
    assert_eq!(initial.name_is_custom, Some(false));
    assert_eq!(
        pane.agent_terminal.as_ref().unwrap().phase,
        AgentTerminalPhase::Starting
    );
    let mut observer = fixture.browser(&initial.id, &pane.id).await;
    assert_eq!(
        control(&mut observer, true).await,
        TerminalControlState::Observing
    );
    observer
        .send(tungstenite::Message::Binary(
            b"must not reach agent\r".to_vec().into(),
        ))
        .await
        .unwrap();
    observer
        .send(tungstenite::Message::Text(
            json!({"type":"resize","cols":30,"rows":10})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let created = creation.await.unwrap();
    assert_eq!(
        created.state.phase,
        AgentTerminalPhase::Ready,
        "{:?}",
        created
    );
    assert!(
        serde_json::to_value(&created)
            .unwrap()
            .get("session_id")
            .is_none()
    );
    assert!(created.state.task_submitted);
    let runtime = fixture
        .state
        .terminals
        .inner
        .terminald
        .get(&pane.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((runtime.cols, runtime.rows), (COLS, ROWS));
    let mut viewer = fixture.browser(&initial.id, &pane.id).await;
    assert_eq!(
        control(&mut viewer, false).await,
        TerminalControlState::Observing
    );
    assert_eq!(
        control(&mut viewer, true).await,
        TerminalControlState::Claimed
    );
    let result = fixture
        .client
        .post_json::<_, Value>(
            &format!("/v1/agents/{}/submit", pane.id),
            &AgentTerminalSubmit {
                task: "blocked".into(),
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(TerminaldClientError::HttpStatus {
            status: StatusCode::CONFLICT,
            ..
        })
    ));
    viewer.close(None).await.unwrap();
    drop(viewer);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if fixture
                .state
                .terminals
                .agent_operation(&pane.id)
                .unwrap()
                .try_write_owned()
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let submitted: AgentTerminalInfo = fixture
        .client
        .post_json(
            &format!("/v1/agents/{}/submit", pane.id),
            &AgentTerminalSubmit {
                task: "second task".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(submitted.pane_id, created.pane_id);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let inputs = std::fs::read_to_string(&fixture.log).unwrap_or_default();
            if inputs.lines().count() >= 2 {
                let inputs: Vec<String> = inputs
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(inputs, vec![&first_task, "second task"]);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    observer.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn rebuild_interrupted_cli_agent_reinitializes_without_replaying_task() {
    let fixture = Fixture::new("ready").await;
    let mut request = fixture.request();
    request.task = Some("original task must only run once".into());
    let original: AgentTerminalInfo = fixture
        .client
        .post_json("/v1/agents", &request)
        .await
        .unwrap();
    assert_eq!(original.state.phase, AgentTerminalPhase::Ready);
    let manager = &fixture.state.terminals;
    let before = manager.get_snapshot(&original.tab_id).unwrap();
    let before_runtime = manager
        .inner
        .terminald
        .get(&original.pane_id)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !std::fs::read_to_string(&fixture.log)
            .unwrap_or_default()
            .contains("original task")
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    // Simulates an agent runtime lost across a daemon restart.
    manager
        .inner
        .terminald
        .delete(&original.pane_id)
        .await
        .unwrap();
    let app = crate::build_router(fixture.state.clone());
    let payload = post(
        &app,
        &format!("/api/terminals/{}/rebuild", original.tab_id),
        json!({}),
    )
    .await;
    let rebuilt: TerminalTab = serde_json::from_value(payload).unwrap();
    assert_eq!(rebuilt.id, before.id);
    assert_ne!(rebuilt.panes[0].id, original.pane_id);
    assert_eq!(
        rebuilt.panes[0].agent_terminal.as_ref().unwrap().phase,
        AgentTerminalPhase::Starting
    );
    assert_eq!(rebuilt.panes[0].cwd, before.panes[0].cwd);
    let runtime = manager
        .inner
        .terminald
        .get(&rebuilt.panes[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(runtime.spec(), before_runtime.spec());
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let info = manager.cli_agent(&rebuilt.panes[0].id).unwrap();
            if info.state.phase != AgentTerminalPhase::Starting {
                assert_eq!(info.state.phase, AgentTerminalPhase::Ready, "{info:?}");
                assert!(!info.state.task_submitted);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&fixture.log)
            .unwrap()
            .lines()
            .count(),
        1
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/terminals/{}/rebuild", original.tab_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    fixture.stop().await;
}

#[tokio::test]
async fn reuses_prepared_worktree_without_reset() {
    let fixture = Fixture::new("ready").await;
    let worktree = fixture.directory.path().join("feature");
    let output = tokio::process::Command::new("git")
        .args(["worktree", "add", "-b", "feature"])
        .arg(&worktree)
        .arg("HEAD")
        .current_dir(&fixture.repo)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::write(worktree.join("keep.txt"), "local changes").unwrap();
    let mut request = fixture.request();
    request.agent = "traecli".into();
    request.cwd = worktree.join(".").to_string_lossy().into_owned();
    request.task = Some("task immediately after input readiness".into());
    let first: AgentTerminalInfo = fixture
        .client
        .post_json("/v1/agents", &request)
        .await
        .unwrap();
    assert_eq!(first.state.phase, AgentTerminalPhase::Ready, "{first:?}");
    assert_eq!(Path::new(&first.cwd), worktree.canonicalize().unwrap());
    let second: AgentTerminalInfo = fixture
        .client
        .post_json("/v1/agents", &request)
        .await
        .unwrap();
    assert_eq!(second.state.phase, AgentTerminalPhase::Ready);
    assert_ne!(first.pane_id, second.pane_id);
    tokio::time::timeout(Duration::from_secs(3), async {
        while std::fs::read_to_string(&fixture.log)
            .unwrap_or_default()
            .lines()
            .count()
            < 2
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let inputs = std::fs::read_to_string(&fixture.log).unwrap();
    let inputs: Vec<String> = inputs
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(inputs, vec![request.task.as_deref().unwrap(); 2]);
    assert_eq!(
        std::fs::read_to_string(Path::new(&request.cwd).join("keep.txt")).unwrap(),
        "local changes"
    );
    let branch = tokio::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(&worktree)
        .output()
        .await
        .unwrap();
    assert!(branch.status.success());
    assert_eq!(String::from_utf8(branch.stdout).unwrap().trim(), "feature");
    let tabs = fixture
        .state
        .terminals
        .list(Some(&first.cwd))
        .await
        .unwrap()
        .tabs;
    assert_eq!(tabs.len(), 2);
    assert!(tabs.iter().any(|tab| tab.id == first.tab_id));
    assert!(tabs.iter().any(|tab| tab.id == second.tab_id));
    let closed = fixture
        .state
        .terminals
        .delete_workspace(&first.cwd)
        .await
        .unwrap();
    assert_eq!(closed, (0, 2));
    assert!(
        fixture
            .state
            .terminals
            .list(Some(&first.cwd))
            .await
            .unwrap()
            .tabs
            .is_empty()
    );
    for pane_id in [&first.pane_id, &second.pane_id] {
        assert!(
            fixture
                .state
                .terminals
                .inner
                .terminald
                .get(pane_id)
                .await
                .unwrap()
                .is_none()
        );
    }
    fixture.stop().await;
}

#[tokio::test]
async fn missing_worktree_is_rejected_without_creating_directories_or_panes() {
    let fixture = Fixture::new("ready").await;
    let parent = fixture.directory.path().join("missing-worktrees");
    let mut request = fixture.request();
    request.cwd = parent.join("feature").to_string_lossy().into_owned();
    request.task = Some("must not run".into());
    let result = fixture
        .client
        .post_json::<_, AgentTerminalInfo>("/v1/agents", &request)
        .await;
    assert!(
        matches!(result,
            Err(aow_terminald_client::TerminaldClientError::HttpStatus { status, .. })
                if status == StatusCode::BAD_REQUEST
        ),
        "{result:?}"
    );
    assert!(!parent.exists());
    let agents: Value = fixture.client.get_json("/v1/agents").await.unwrap();
    assert_eq!(agents["items"], json!([]));
    assert!(
        fixture
            .state
            .terminals
            .inner
            .terminald
            .list()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!fixture.log.exists());
    fixture.stop().await;
}

#[tokio::test]
async fn create_rejects_unregistered_or_mismatched_project_worktrees() {
    let fixture = Fixture::new("ready").await;
    let nested = fixture.repo.join("nested-project");
    let output = tokio::process::Command::new("git")
        .args(["clone", "--quiet", "--no-hardlinks"])
        .arg(&fixture.repo)
        .arg(&nested)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut request = fixture.request();
    request.cwd = nested.to_string_lossy().into_owned();
    request.task = Some("must not run".into());
    let result = fixture
        .client
        .post_json::<_, AgentTerminalInfo>("/v1/agents", &request)
        .await;
    assert!(
        matches!(result,
            Err(aow_terminald_client::TerminaldClientError::HttpStatus { status, .. })
                if status == StatusCode::BAD_REQUEST
        ),
        "{result:?}"
    );

    let app = crate::build_router(fixture.state.clone());
    let nested_project = post(
        &app,
        "/api/aow/projects",
        json!({"path":nested, "notes_path":fixture.directory.path().join("nested-notes")}),
    )
    .await;
    let nested_id = nested_project["id"].as_str().unwrap();
    let subdirectory = fixture.repo.join("src");
    std::fs::create_dir(&subdirectory).unwrap();
    for (project_id, cwd, expected) in [
        (
            fixture.project_id.as_str(),
            &nested,
            StatusCode::BAD_REQUEST,
        ),
        (nested_id, &fixture.repo, StatusCode::BAD_REQUEST),
        (
            fixture.project_id.as_str(),
            &subdirectory,
            StatusCode::BAD_REQUEST,
        ),
        ("missing-project", &fixture.repo, StatusCode::NOT_FOUND),
    ] {
        request.project_id = project_id.into();
        request.cwd = cwd.to_string_lossy().into_owned();
        let result = fixture
            .client
            .post_json::<_, AgentTerminalInfo>("/v1/agents", &request)
            .await;
        assert!(
            matches!(result,
                Err(aow_terminald_client::TerminaldClientError::HttpStatus { status, .. })
                    if status == expected
            ),
            "{project_id} {}: {result:?}",
            cwd.display()
        );
    }
    let agents: Value = fixture.client.get_json("/v1/agents").await.unwrap();
    assert_eq!(agents["items"], json!([]));
    assert!(
        fixture
            .state
            .terminals
            .inner
            .terminald
            .list()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!fixture.log.exists());
    fixture.stop().await;
}

#[tokio::test]
async fn startup_without_task_is_ready_without_writing_any_input() {
    let fixture = Fixture::new("ready").await;
    for agent in ["codex", "traecli"] {
        let mut request = fixture.request();
        request.agent = agent.into();
        let result: AgentTerminalInfo = fixture
            .client
            .post_json("/v1/agents", &request)
            .await
            .unwrap();
        assert_eq!(result.state.phase, AgentTerminalPhase::Ready, "{result:?}");
        assert!(!result.state.task_submitted);
        assert!(
            serde_json::to_value(result)
                .unwrap()
                .get("session_id")
                .is_none()
        );
        assert!(
            !fixture.log.exists(),
            "startup must not submit probe commands"
        );
        assert!(
            !fixture.directory.path().join("input.jsonl.raw").exists(),
            "startup must not write any input, including Escape"
        );
    }
    fixture.stop().await;
}

#[tokio::test]
async fn startup_timeout_preserves_pane_and_never_submits_task() {
    let fixture = Fixture::new("loading").await;
    let mut request = fixture.request();
    request.timeout_seconds = 2;
    request.task = Some("must not run".into());
    let result: AgentTerminalInfo = fixture
        .client
        .post_json("/v1/agents", &request)
        .await
        .unwrap();
    assert_eq!(result.state.phase, AgentTerminalPhase::Failed);
    assert!(result.state.error.unwrap().contains("timed out"));
    assert!(!result.state.task_submitted);
    assert!(
        fixture
            .state
            .terminals
            .inner
            .terminald
            .get(&result.pane_id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(!fixture.log.exists());
    assert!(!fixture.directory.path().join("input.jsonl.raw").exists());
    let operation = fixture
        .state
        .operations
        .snapshot()
        .operations
        .pop()
        .unwrap();
    assert_eq!(operation.outcome, Some(Outcome::Failed));
    assert!(operation.message.contains("timed out"));
    fixture.stop().await;
}

#[tokio::test]
async fn disconnected_cli_creation_still_finishes_and_logs_without_task_contents() {
    let fixture = Fixture::new("ready").await;
    let mut request = fixture.request();
    request.task = Some("private task body should never enter operation history".into());
    let client = fixture.client.clone();
    let creation = tokio::spawn(async move {
        client
            .post_json::<_, AgentTerminalInfo>("/v1/agents", &request)
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while fixture.state.operations.snapshot().operations.is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    creation.abort();
    let operation = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(operation) = fixture
                .state
                .operations
                .snapshot()
                .operations
                .into_iter()
                .find(|operation| operation.outcome.is_some())
            {
                break operation;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(operation.outcome, Some(Outcome::Succeeded));
    assert_eq!(operation.source, "cli");
    assert!(
        operation
            .resource
            .unwrap()
            .starts_with("/aow/tabs/terminal/")
    );
    let reader = aow_operation_log::Reader::new(fixture.directory.path().join("operation-logs"));
    let page = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = reader.read(&Default::default()).unwrap();
            if page
                .items
                .first()
                .is_some_and(|record| record.event == "finished")
            {
                break page;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        page.items
            .iter()
            .all(|record| record.operation_id == operation.id)
    );
    assert!(
        page.items
            .iter()
            .any(|record| record.message == "提交初始任务")
    );
    assert!(
        !serde_json::to_string(&page)
            .unwrap()
            .contains("private task body")
    );
    fixture.stop().await;
}

#[tokio::test]
#[ignore = "requires installed/authenticated Codex and TraeCode CLI; only observes startup"]
async fn native_startup_probes() {
    let fixture = Fixture::new("ready").await;
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for name in ["codex", "traecli"] {
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|path| path.join(name))
            .find(|path| path.is_file())
            .unwrap();
        let tab = fixture
            .state
            .terminals
            .create_agent_with_state(
                CreateTerminalRequest {
                    name: None,
                    cwd: Some(cwd.clone()),
                    workspace_root: Some(cwd.clone()),
                    shell: None,
                    agent_id: Some(name.into()),
                    resume_session_id: None,
                    rows: Some(ROWS),
                    cols: Some(COLS),
                },
                AgentLaunch {
                    agent_type: aow_agents::launch::AgentType::from_id(name).unwrap(),
                    display_name: name.into(),
                    executable: executable.to_string_lossy().into_owned(),
                    args: vec![],
                    env: Default::default(),
                },
                Some(AgentTerminalState {
                    phase: AgentTerminalPhase::Starting,
                    error: None,
                    task_submitted: false,
                }),
            )
            .await
            .unwrap();
        let pane_id = &tab.panes[0].id;
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            fixture.state.terminals.initialize_agent(
                pane_id,
                Agent::from_id(name).unwrap().interactive().unwrap(),
                None,
                None,
            ),
        )
        .await;
        assert!(
            matches!(&result, Ok(Ok(()))),
            "{name} startup probe: {result:?}"
        );
        let mut connection =
            AgentConnection::claim(&fixture.state.terminals.inner.terminald, pane_id)
                .await
                .unwrap();
        connection.write("\x04".into()).await.unwrap();
        fixture
            .state
            .terminals
            .inner
            .terminald
            .delete(pane_id)
            .await
            .unwrap();
    }
    fixture.stop().await;
}
