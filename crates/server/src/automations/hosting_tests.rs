use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use tower::ServiceExt;

// Real PTY and real native history; the manual task uses the production Runner.
// The scheduler command is a no-op so the test can control run completion without
// registering a job with the host's launchd/systemd.
const AGENT: &str = r#"
import os, sys, json, sqlite3, time, tty
from pathlib import Path
root = Path(os.environ.get('CODEX_HOME') or str(Path(os.environ['HOME']) / '.codex'))
(root / 'sessions').mkdir(parents=True, exist_ok=True)
db = sqlite3.connect(root / 'state_5.sqlite', timeout=10)
db.execute('CREATE TABLE IF NOT EXISTS threads (id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT, title TEXT, created_at INTEGER, updated_at INTEGER, updated_at_ms INTEGER, archived INTEGER, first_user_message TEXT, thread_source TEXT, source TEXT, agent_nickname TEXT, agent_role TEXT)')
def event(path, kind, **values):
    with open(path, 'a') as f: f.write(json.dumps({'type':'event_msg', 'payload':{'type':kind, **values}}) + '\n')
def session(sid, title, prompt, conclusion):
    path = root / 'sessions' / (sid + '.jsonl')
    event(path, 'task_started', turn_id='turn-one')
    event(path, 'user_message', message=prompt)
    if conclusion is not None: event(path, 'task_complete', turn_id='turn-one', last_agent_message=conclusion)
    db.execute('INSERT INTO threads VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)', (sid,str(path),os.getcwd(),title,1,1,1,0,prompt,'cli','cli','',''))
    db.commit()
    return path
config = json.loads((root / 'test-config.json').read_text())
if 'exec' in sys.argv:
    prompt = sys.stdin.read()
    if config.get('registry_path'):
        registry_path = Path(config['registry_path'])
        registry = json.loads(registry_path.read_text())
        registry['items'][0]['env'] = config['next_environment']
        registry_path.write_text(json.dumps(registry))
    sid = 'review-' + str(os.getpid())
    counter = root / 'review-count'
    count = int(counter.read_text()) + 1 if counter.exists() else 1
    counter.write_text(str(count))
    (root / 'review-prompt').write_text(prompt)
    if config.get('fail_review'): sys.exit(1)
    conclusion = 'Review feedback: fix the missing assertion.\n保持多行。'
    if config['finish_on_review'] == count: conclusion = '审查通过。\n[AOW_HOSTING_DONE]\n'
    session(sid, 'Review run', prompt, conclusion)
    print('session id: ' + sid, file=sys.stderr)
    print('Review completed')
    sys.exit(0)
tty.setraw(0)
os.chdir('frontend')
path = session('source-session', 'Hosting integration source', 'Implement feature', None if config.get('source_running') else 'Implementation finished')
os.write(1, b'\x1b]2;Hosting integration source\x07Ready\r\n> ')
buf = b''
turn_index = 1
while True:
    ch = os.read(0, 1)
    if not ch: break
    if ch != b'\r':
        buf += ch
        continue
    prompt = buf.decode().replace('\x1b[200~','').replace('\x1b[201~','')
    buf = b''
    with open(root / 'received.jsonl', 'a') as f: f.write(json.dumps(prompt) + '\n')
    turn_index += 1
    turn_id = 'turn-' + str(turn_index)
    event(path, 'task_started', turn_id=turn_id)
    event(path, 'user_message', message=prompt)
    if config['complete_feedback']: event(path, 'task_complete', turn_id=turn_id, last_agent_message='Fixed')
    os.write(1, b'Working\r\n')
"#;

async fn request(app: &Router, method: &str, path: &str, body: Value) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    assert!(
        status.is_success(),
        "{status}: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

struct DaemonGuard(Option<tokio::sync::oneshot::Sender<()>>);
impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.0.take() {
            let _ = stop.send(());
        }
    }
}

#[tokio::test]
async fn hosting_executes_review_in_source_subdirectory_and_feeds_original_terminal_once() {
    run_hosting_case(HostingCase::default()).await;
}

#[tokio::test]
async fn hosting_finish_marker_stops_without_feedback_and_survives_restart() {
    run_hosting_case(HostingCase {
        max_inputs: Some(2),
        finish_on_review: Some(1),
        complete_feedback: true,
        run_on_enable: Some(true),
        ..HostingCase::default()
    })
    .await;
}

#[tokio::test]
async fn hosting_limits_feedback_but_runs_a_final_review() {
    run_hosting_case(HostingCase {
        max_inputs: Some(2),
        complete_feedback: true,
        ..HostingCase::default()
    })
    .await;
}

#[tokio::test]
async fn hosting_final_review_can_pass_after_all_inputs_are_used() {
    run_hosting_case(HostingCase {
        max_inputs: Some(2),
        finish_on_review: Some(3),
        complete_feedback: true,
        run_on_enable: Some(true),
        ..HostingCase::default()
    })
    .await;
}

#[tokio::test]
async fn hosting_can_wait_for_a_new_completion_without_replaying_history() {
    run_hosting_case(HostingCase {
        run_on_enable: Some(false),
        ..HostingCase::default()
    })
    .await;
}

#[tokio::test]
async fn hosting_recovers_initial_review_without_dispatching_it_again() {
    run_hosting_case(HostingCase {
        restart: true,
        ..HostingCase::default()
    })
    .await;
}

#[tokio::test]
async fn hosting_recovers_waiting_without_starting_an_initial_review() {
    run_hosting_case(HostingCase {
        run_on_enable: Some(false),
        restart: true,
        ..HostingCase::default()
    })
    .await;
}

#[tokio::test]
async fn hosting_initial_review_failure_stops_without_feedback() {
    run_hosting_case(HostingCase {
        fail_review: true,
        ..HostingCase::default()
    })
    .await;
}

#[tokio::test]
async fn hosted_result_uses_original_codex_home_when_registration_changes_during_run() {
    review_environment_case(false).await;
}

#[tokio::test]
async fn hosted_result_uses_original_home_override_when_registration_changes_during_run() {
    review_environment_case(true).await;
}

async fn review_environment_case(home_only: bool) {
    let directory = tempfile::tempdir().unwrap();
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
        assert!(
            tokio::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .await
                .unwrap()
                .status
                .success()
        );
    }
    let mut state = AppState::with_state_dir(
        PathBuf::new(),
        directory.path().join("state"),
        directory.path().join("unused.sock"),
    )
    .unwrap();
    state.auth = crate::auth::AuthService::disabled();
    state.automations.as_mut().unwrap().scheduler = Scheduler {
        platform: aow_automations::scheduler::Platform::Systemd,
        runner: "/usr/bin/true".into(),
        directory: directory.path().join("units"),
        manager_command: "/usr/bin/true".into(),
        dispatch_command: "/usr/bin/true".into(),
    };
    let manager = state.automations.clone().unwrap();
    let app = crate::build_router(state);
    request(
        &app,
        "PUT",
        "/api/aow/settings",
        json!({"execution_path":["/usr/bin", "/bin"]}),
    )
    .await;
    let project = request(&app, "POST", "/api/aow/projects", json!({"path":repo})).await;
    let home = directory.path().join("reviewer-home");
    let root = if home_only {
        home.join(".codex")
    } else {
        directory.path().join("reviewer-codex")
    };
    std::fs::create_dir_all(&root).unwrap();
    let registry_path = manager
        .store
        .config_dir
        .join(aow_agents::environment::AGENTS_FILE);
    std::fs::write(root.join("test-config.json"), json!({
        "finish_on_review":null, "registry_path":registry_path,
        "next_environment":{"HOME":directory.path().join("changed-home"),"CODEX_HOME":directory.path().join("changed-codex")},
    }).to_string()).unwrap();
    let script = directory.path().join("codex");
    std::fs::write(&script, AGENT).unwrap();
    request(&app, "POST", "/api/aow/agents", json!({"id":"codex","agent_type":"codex","display_name":"Review fixture","command":"/usr/bin/python3","args":[script],"env":{"HOME":home,"CODEX_HOME":if home_only { String::new() } else { root.to_string_lossy().into_owned() }}})).await;
    let task = request(&app, "POST", "/api/aow/automations", json!({
        "project_id":project["id"],"name":"Review","kind":"manual","prompt":"Review",
        "agent":"codex","workspace_mode":"existing","workspace_path":repo,"base_branch":"","cron":"",
        "max_concurrent_runs":1,"enabled":true
    })).await;
    let task_id = task["id"].as_str().unwrap();
    let run_id = aow_automations::store::new_run_id();
    assert_eq!(
        aow_automations::runner::run(
            &manager.store,
            task_id,
            Some(run_id.clone()),
            RunSource::Manual
        )
        .await
        .unwrap(),
        aow_automations::RunStatus::Completed
    );
    let changed: Value = serde_json::from_slice(&std::fs::read(registry_path).unwrap()).unwrap();
    assert_eq!(
        changed["items"][0]["env"]["CODEX_HOME"],
        json!(directory.path().join("changed-codex"))
    );
    let run = manager.store.read_run(task_id, &run_id).unwrap().unwrap();
    let expected = "Review feedback: fix the missing assertion.\n保持多行。";
    // The runner has already saved the result; no consumer has read it yet.
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(manager.store.read_run_result(&run).unwrap(), expected);
}

#[derive(Default)]
struct HostingCase {
    max_inputs: Option<u32>,
    finish_on_review: Option<u32>,
    complete_feedback: bool,
    run_on_enable: Option<bool>,
    restart: bool,
    fail_review: bool,
}

async fn run_hosting_case(case: HostingCase) {
    let HostingCase {
        max_inputs,
        finish_on_review,
        complete_feedback,
        run_on_enable,
        restart,
        fail_review,
    } = case;
    let immediate = run_on_enable.unwrap_or(true);
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let source_cwd = repo.join("frontend");
    std::fs::create_dir(&source_cwd).unwrap();
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
        assert!(
            tokio::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .await
                .unwrap()
                .status
                .success()
        );
    }
    let socket = directory.path().join("terminald/terminal.sock");
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let _guard = DaemonGuard(Some(stop));
    let daemon_socket = socket.clone();
    let daemon = tokio::spawn(aow_terminald::run_with_shutdown(daemon_socket, async {
        let _ = stopped.await;
    }));
    let native = aow_terminald_client::TerminaldClient::new(socket.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        while native.health().await.is_err() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let state_dir = directory.path().join("state");
    let mut state = AppState::with_state_dir(PathBuf::new(), state_dir.clone(), socket).unwrap();
    state.auth = crate::auth::AuthService::disabled();
    state.automations.as_mut().unwrap().scheduler = Scheduler {
        platform: aow_automations::scheduler::Platform::Systemd,
        runner: "/usr/bin/true".into(),
        directory: directory.path().join("units"),
        manager_command: "/usr/bin/true".into(),
        dispatch_command: "/usr/bin/true".into(),
    };
    let mut app = crate::build_router(state.clone());
    request(
        &app,
        "PUT",
        "/api/aow/settings",
        json!({"execution_path":["/usr/bin", "/bin"]}),
    )
    .await;
    let project = request(&app, "POST", "/api/aow/projects", json!({"path":repo})).await;
    let home = directory.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("test-config.json"),
        json!({"finish_on_review": finish_on_review, "complete_feedback": complete_feedback, "fail_review": fail_review, "source_running": run_on_enable.is_none()})
            .to_string(),
    )
    .unwrap();
    let script = directory.path().join("codex");
    std::fs::write(&script, AGENT).unwrap();
    request(&app, "POST", "/api/aow/agents", json!({"id":"codex","agent_type":"codex","display_name":"Codex fixture","command":"/usr/bin/python3","args":[script],"env":{"CODEX_HOME":home}})).await;
    let task = request(&app, "POST", "/api/aow/automations", json!({
        "project_id":project["id"],"name":"Review","kind":"manual","prompt":"Review {{conclusion}} in {{workspace}} session {{session_id}} turn {{turn_id}}",
        "prompt_bindings":[{"name":"conclusion","placeholder":"{{conclusion}}","start":7,"end":21},{"name":"workspace","placeholder":"{{workspace}}","start":25,"end":38},{"name":"session_id","placeholder":"{{session_id}}","start":47,"end":61},{"name":"turn_id","placeholder":"{{turn_id}}","start":67,"end":78}],
        "agent":"codex","workspace_mode":"dynamic","workspace_path":"","base_branch":"","cron":"",
        "max_concurrent_runs":1,"enabled":true
    })).await;
    let tab = request(
        &app,
        "POST",
        "/api/terminals",
        json!({"cwd":repo,"workspace_root":repo,"agent_id":"codex"}),
    )
    .await;
    let pane_id = tab["panes"][0]["id"].as_str().unwrap();
    let tab_id = tab["id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let detected = native.agents().await.unwrap();
            if detected
                .titles
                .get(pane_id)
                .is_some_and(|title| title == "Hosting integration source")
                && detected.processes.get(pane_id).is_some_and(|process| {
                    std::path::Path::new(&process.cwd) == source_cwd.canonicalize().unwrap()
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    let endpoint = format!("/api/terminals/{tab_id}/panes/{pane_id}/hosting");
    // With no future native completions, enable page notifications to catch any
    // fabricated first-review event. Other cases verify hosting still consumes
    // real completions when all user-facing notifications are disabled.
    let notify_page = run_on_enable.is_none() && !complete_feedback;
    request(
        &app,
        "PUT",
        "/api/aow/notification-settings",
        json!({
            "section":"notifications", "agent_task_completed":{"enabled":notify_page,"channels":if notify_page { vec!["page"] } else { vec![] }}
        }),
    )
    .await;
    let mut page_notifications = state.terminals.subscribe_task_stops();
    state.start_agent_notifications();
    let invalid = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(&endpoint)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"task_id":task["id"],"revision":task["revision"],"max_inputs":0})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), axum::http::StatusCode::BAD_REQUEST);
    let mut body = json!({"task_id":task["id"],"revision":task["revision"]});
    if let Some(limit) = max_inputs {
        body["max_inputs"] = json!(limit);
    }
    if let Some(immediate) = run_on_enable {
        body["run_on_enable"] = json!(immediate);
    }
    let task_id = task["id"].as_str().unwrap();
    let store = state.automations.as_ref().unwrap().store.clone();
    // Enable on a disposable runtime to simulate a real server restart: dropping
    // it aborts the worker, while terminald and the persisted state survive.
    let hosted = if restart {
        let enable_app = app.clone();
        let endpoint = endpoint.clone();
        let tab_endpoint = format!("/api/terminals/{tab_id}");
        let pending = store.root.join("pending").join(task_id);
        let hosted = tokio::task::spawn_blocking(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    request(&enable_app, "PUT", &endpoint, body).await;
                    if immediate {
                        tokio::time::timeout(Duration::from_secs(10), async {
                            while !pending.exists()
                                || std::fs::read_dir(&pending).unwrap().count() == 0
                            {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                            }
                        })
                        .await
                        .unwrap();
                    }
                    request(&enable_app, "GET", &tab_endpoint, Value::Null).await
                })
        })
        .await
        .unwrap();
        let mut restored = AppState::with_state_dir(
            PathBuf::new(),
            state_dir.clone(),
            directory.path().join("terminald/terminal.sock"),
        )
        .unwrap();
        restored.auth = crate::auth::AuthService::disabled();
        restored.automations.as_mut().unwrap().scheduler =
            state.automations.as_ref().unwrap().scheduler.clone();
        state = restored;
        page_notifications = state.terminals.subscribe_task_stops();
        state.start_agent_notifications();
        crate::terminal::hosting::recover(state.clone());
        app = crate::build_router(state.clone());
        hosted
    } else {
        request(&app, "PUT", &endpoint, body).await
    };
    assert!(matches!(
        hosted["panes"][0]["hosting"]["phase"].as_str(),
        Some("waiting" | "reviewing")
    ));
    assert_eq!(hosted["panes"][0]["hosting"]["input_count"], 0);
    assert_eq!(
        hosted["panes"][0]["hosting"]["max_inputs"],
        max_inputs.unwrap_or(3)
    );
    // Omitted run_on_enable leaves the source task running. Explicit options
    // have an old completion: neither the worker nor the shared notification
    // reader may replay it or use its values for the initial review.
    tokio::time::sleep(Duration::from_millis(3300)).await;
    let waiting = request(
        &app,
        "GET",
        &format!("/api/terminals/{tab_id}"),
        Value::Null,
    )
    .await;
    let initial = &waiting["panes"][0]["hosting"];
    assert_eq!(
        initial["phase"],
        if immediate { "reviewing" } else { "waiting" }
    );
    assert_eq!(initial["run_id"].is_string(), immediate);
    assert!(initial["source_turn_id"].is_null());
    assert_eq!(initial["input_count"], 0);
    if restart {
        assert_eq!(*initial, hosted["panes"][0]["hosting"]);
    }
    assert!(!home.join("review-count").exists());
    // Explicit false needs a new completion; explicit true receives the same
    // event during its initial review and must not dispatch a second run.
    // Omitted run_on_enable completes the first review with no new event at all.
    if run_on_enable.is_some() {
        use std::io::Write;
        let mut transcript = std::fs::OpenOptions::new()
            .append(true)
            .open(home.join("sessions/source-session.jsonl"))
            .unwrap();
        for payload in [
            json!({"type":"task_started","turn_id":"after-enable"}),
            json!({"type":"user_message","message":"Implement the next change"}),
            json!({"type":"task_complete","turn_id":"after-enable","last_agent_message":"Implementation finished"}),
            json!({"type":"task_complete","turn_id":"after-enable","last_agent_message":"Implementation finished"}),
        ] {
            writeln!(
                transcript,
                "{}",
                json!({"type":"event_msg","payload":payload})
            )
            .unwrap();
        }
    }
    // Let the shared detector deliver concurrent/duplicate events before the
    // first review completes, exercising the worker's Reviewing guard.
    if run_on_enable == Some(true) {
        tokio::time::sleep(Duration::from_millis(3300)).await;
        let current = request(
            &app,
            "GET",
            &format!("/api/terminals/{tab_id}"),
            Value::Null,
        )
        .await;
        assert_eq!(current["panes"][0]["hosting"], *initial);
    }
    let received = home.join("received.jsonl");
    let mut runs = Vec::new();
    let final_hosting = tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let current = request(
                &app,
                "GET",
                &format!("/api/terminals/{tab_id}"),
                Value::Null,
            )
            .await;
            let hosting = &current["panes"][0]["hosting"];
            if fail_review && hosting["phase"] == "failed" {
                break hosting.clone();
            }
            assert!(hosting["error"].is_null(), "{hosting}");
            assert!(hosting["input_count"].as_u64().unwrap() <= u64::from(max_inputs.unwrap_or(3)));
            if hosting["phase"] == "completed"
                || hosting["phase"] == "limit_reached"
                || (!complete_feedback && hosting["input_count"] == 1 && received.exists())
            {
                break hosting.clone();
            }
            if let Some(run_id) = hosting["run_id"].as_str() {
                let pending = store
                    .root
                    .join("pending")
                    .join(task_id)
                    .join(format!("{run_id}.json"));
                if !runs.iter().any(|id| id == run_id) && pending.exists() {
                    assert_eq!(
                        std::fs::read_dir(pending.parent().unwrap())
                            .unwrap()
                            .count(),
                        1
                    );
                    let request: Value =
                        serde_json::from_slice(&std::fs::read(&pending).unwrap()).unwrap();
                    assert_eq!(request["hosted"], true);
                    assert!(request.get("hosting_context").is_none());
                    if runs.is_empty() {
                        assert_eq!(request["variables"]["session_id"], "source-session");
                        assert_eq!(
                            request["variables"]["turn_id"],
                            if immediate {
                                "（无）"
                            } else {
                                "after-enable"
                            }
                        );
                        assert_eq!(
                            request["variables"]["conclusion"],
                            if immediate {
                                "（无）"
                            } else {
                                "Implementation finished"
                            }
                        );
                        if immediate {
                            assert_eq!(initial["run_id"], run_id);
                        }
                    }
                    runs.push(run_id.to_owned());
                    assert_eq!(
                        aow_automations::runner::run(
                            &store,
                            task_id,
                            Some(run_id.into()),
                            RunSource::Manual
                        )
                        .await
                        .unwrap(),
                        if fail_review {
                            aow_automations::RunStatus::Failed
                        } else {
                            aow_automations::RunStatus::Completed
                        }
                    );
                    if runs.len() == 1 {
                        let prompt = std::fs::read_to_string(home.join("review-prompt")).unwrap();
                        assert!(prompt.contains(if immediate {
                            "Review （无） in "
                        } else {
                            "Review Implementation finished in "
                        }));
                        assert!(prompt.contains(if immediate {
                            "session source-session turn （无）"
                        } else {
                            "session source-session turn after-enable"
                        }));
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    if fail_review {
        assert_eq!(final_hosting["input_count"], 0);
        assert!(final_hosting["error"].is_string());
        assert!(!received.exists());
        assert_eq!(runs.len(), 1);
        assert_eq!(store.runs(task_id, None, 10).unwrap().len(), 1);
        drop(_guard);
        daemon.await.unwrap().unwrap();
        return;
    }
    let expected_inputs = if complete_feedback {
        finish_on_review
            .map(|count| count - 1)
            .unwrap_or(max_inputs.unwrap())
    } else {
        1
    };
    assert_eq!(final_hosting["input_count"], expected_inputs);
    assert_eq!(
        final_hosting["phase"],
        if finish_on_review.is_some() {
            "completed"
        } else if complete_feedback {
            "limit_reached"
        } else {
            "waiting"
        }
    );
    let inputs: Vec<String> = std::fs::read_to_string(&received)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        inputs,
        vec![
            "Review feedback: fix the missing assertion.\n保持多行。";
            expected_inputs as usize
        ]
    );
    let prompt = std::fs::read_to_string(home.join("review-prompt")).unwrap();
    assert!(prompt.starts_with("你正在执行一次审查任务，请遵循任务原有的执行和输出要求。"));
    assert!(prompt.contains("单独追加标记：[AOW_HOSTING_DONE]。\n\n审查任务：\nReview "));
    assert!(!prompt.contains("审查上下文："));
    assert!(!prompt.contains("hosting-context.json"));
    assert!(!prompt.contains("Terminal"));
    let run_id = &runs[0];
    let run = store.read_run(task_id, run_id).unwrap().unwrap();
    assert_eq!(
        run.workspace_path.unwrap(),
        source_cwd.canonicalize().unwrap()
    );
    assert!(prompt.contains(source_cwd.canonicalize().unwrap().to_str().unwrap()));
    assert!(page_notifications.try_recv().is_err());
    for run_id in &runs {
        assert!(
            !store
                .run_path(task_id, run_id)
                .unwrap()
                .join("hosting-context.json")
                .exists()
        );
    }
    assert_eq!(
        store
            .get_task(task_id)
            .unwrap()
            .input
            .workspace
            .workspace_mode,
        WorkspaceMode::Dynamic
    );
    if complete_feedback {
        // Restoring a stopped host must keep its count and outcome without
        // claiming control, dispatching another run or replaying the feedback.
        let mut restored = AppState::with_state_dir(
            PathBuf::new(),
            state_dir,
            directory.path().join("terminald/terminal.sock"),
        )
        .unwrap();
        restored.automations.as_mut().unwrap().scheduler =
            state.automations.as_ref().unwrap().scheduler.clone();
        restored.auth = crate::auth::AuthService::disabled();
        crate::terminal::hosting::recover(restored.clone());
        let restored_tab = request(
            &crate::build_router(restored),
            "GET",
            &format!("/api/terminals/{tab_id}"),
            Value::Null,
        )
        .await;
        assert_eq!(restored_tab["panes"][0]["hosting"], final_hosting);
    }
    tokio::time::sleep(Duration::from_millis(1800)).await;
    assert_eq!(
        std::fs::read_to_string(received)
            .unwrap_or_default()
            .lines()
            .count(),
        expected_inputs as usize
    );
    assert_eq!(
        store.runs(task_id, None, 10).unwrap().len(),
        if complete_feedback {
            expected_inputs as usize + 1
        } else {
            1
        }
    );
    assert_eq!(
        std::fs::read_dir(store.root.join("pending").join(task_id))
            .unwrap()
            .count(),
        0
    );
    drop(_guard);
    daemon.await.unwrap().unwrap();
}
