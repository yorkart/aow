use super::*;
use aow_automations::{
    RunEvent, RunStatus, runner::initial_run, scheduler::Platform, store::new_run_id,
};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;

async fn call(
    router: &Router,
    method: &str,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    (
        status,
        if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

async fn call_text(router: &Router, path: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 5_000_000).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn api_persists_config_sync_errors_and_history_across_server_restarts() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repo");
    fs::create_dir(&repository).unwrap();
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
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repository)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let fake = directory.path().join("fake-command");
    fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let state_dir = directory.path().join("state");
    let mut state = AppState::with_state_dir(
        directory.path().join("frontend"),
        state_dir.clone(),
        directory.path().join("terminal.sock"),
    )
    .unwrap();
    // This test exercises automation persistence, not the HTTP
    // authentication boundary. Runtime construction remains protected.
    state.auth = crate::auth::AuthService::disabled();
    state.automations.as_mut().unwrap().scheduler = Scheduler {
        platform: Platform::Systemd,
        runner: fake.clone(),
        directory: directory.path().join("units"),
        manager_command: fake.clone(),
        dispatch_command: fake.clone(),
    };
    let manager = state.automations.clone().unwrap();
    let router = crate::build_router(state);
    let (code, settings) = call(&router, "GET", "/api/aow/settings", serde_json::Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert!(!settings["execution_path"].as_array().unwrap().is_empty());
    let paths = serde_json::json!([directory.path(), "/usr/bin", "/bin"]);
    let (code, updated) = call(
        &router,
        "PUT",
        "/api/aow/settings",
        serde_json::json!({"execution_path": paths}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{updated}");
    assert_eq!(updated["execution_path"], paths);
    assert_eq!(updated["notes_base"], settings["notes_base"]);
    let (code, discovered) = call(
        &router,
        "GET",
        "/api/aow/settings/discovered-path",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert!(!discovered.as_array().unwrap().is_empty());
    assert_eq!(
        call(&router, "GET", "/api/aow/settings", serde_json::Value::Null)
            .await
            .1["execution_path"],
        paths
    );
    let (code, _) = call(
        &router,
        "PUT",
        "/api/aow/settings",
        serde_json::json!({"execution_path": ["relative/bin"]}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, project) = call(
        &router,
        "POST",
        "/api/aow/projects",
        serde_json::json!({"path":repository,"notes_path":directory.path().join("notes")}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{project}");
    let (code, agent) = call(
        &router,
        "POST",
        "/api/aow/agents",
        serde_json::json!({"id":"codex","agent_type":"codex","display_name":"Codex test","command":fake}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{agent}");
    let input = serde_json::json!({"name":"Daily review","prompt":"Review recent changes","agent":"codex","project_id":project["id"],"workspace_mode":"new_worktree","workspace_path":repository,"base_branch":"main","cron":"0 9 * * 1-5","max_concurrent_runs":1,"enabled":true,"failure_notification":"feishu"});
    let (code, task) = call(&router, "POST", "/api/aow/automations", input.clone()).await;
    assert_eq!(code, StatusCode::CREATED, "{task}");
    let id = task["id"].as_str().unwrap();
    assert_eq!(id.len(), 8);
    assert!(id.bytes().all(|byte| byte.is_ascii_digit()));
    assert!(task["scheduler_error"].is_null());
    assert_eq!(task["yolo"], true);
    assert_eq!(task["cleanup_worktree"], true);
    assert!(task.get("launch").is_none());
    assert!(task["next_run_at"].is_string());
    assert_eq!(task["max_concurrent_runs"], 1);
    // Synced task preferences remain valid without a locally configured bot.
    assert_eq!(task["failure_notification"], "feishu");
    assert_eq!(
        manager
            .store
            .get_task(id)
            .unwrap()
            .input
            .failure_notification,
        Some(aow_automations::FailureNotification::Feishu)
    );
    assert!(task.get("prevent_overlap").is_none());
    assert!(task["interval_seconds"].is_null());
    let path = format!("/api/aow/automations/{id}");
    assert_eq!(
        call(
            &router,
            "GET",
            &format!(
                "/api/aow/automations?project_id={}",
                project["id"].as_str().unwrap()
            ),
            serde_json::Value::Null,
        )
        .await
        .1[0]["id"],
        id
    );
    assert_eq!(
        call(
            &router,
            "GET",
            "/api/aow/automations?project_id=another-project",
            serde_json::Value::Null,
        )
        .await
        .1,
        serde_json::json!([])
    );
    let persisted = manager.store.get_task(id).unwrap();
    assert!(!persisted.launch.environment.contains_key("PATH"));
    assert!(persisted.launch.environment.keys().all(|key| {
        [
            "CODEX_HOME",
            "CLAUDE_CONFIG_DIR",
            "TRAE_HOME",
            "TRAECLI_HOME",
        ]
        .contains(&key.as_str())
    }));
    assert!(persisted.input.yolo);
    assert!(persisted.input.cleanup_worktree);
    let run_id = new_run_id();
    let mut journal = manager
        .store
        .create_run(
            &initial_run(&persisted, run_id.clone(), RunSource::Manual),
            &persisted,
        )
        .unwrap();
    journal
        .append(&RunEvent::AgentStarted {
            pid: 4242,
            command: vec![
                "/usr/local/bin/codex".into(),
                "exec".into(),
                "--json".into(),
            ],
        })
        .unwrap();
    journal
        .append(&RunEvent::Session {
            session_id: "native-session".into(),
            elapsed_ms: 10,
        })
        .unwrap();
    journal
        .append(&RunEvent::Finished {
            at: Utc::now(),
            status: RunStatus::Completed,
            exit_code: Some(0),
            message: None,
            duration_ms: 50,
        })
        .unwrap();
    fs::write(
        manager
            .store
            .run_output_path(id, &run_id, RunOutput::Stdio)
            .unwrap(),
        "{\"type\":\"thread.started\"}\n",
    )
    .unwrap();
    fs::write(
        manager
            .store
            .run_output_path(id, &run_id, RunOutput::Stderr)
            .unwrap(),
        "warning on stderr\n",
    )
    .unwrap();
    drop(journal);
    let mut restarted = AppState::with_state_dir(
        directory.path().join("frontend"),
        state_dir,
        directory.path().join("terminal.sock"),
    )
    .unwrap();
    restarted.auth = crate::auth::AuthService::disabled();
    restarted.automations.as_mut().unwrap().scheduler = manager.scheduler.clone();
    let router = crate::build_router(restarted);
    let (code, detail) = call(&router, "GET", &path, serde_json::Value::Null).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(detail["last_run"]["session_id"], "native-session");
    assert_eq!(
        detail["last_run"]["agent_command"],
        serde_json::json!(["/usr/local/bin/codex", "exec", "--json"])
    );
    let output_path = format!("{path}/runs/{run_id}/output/stdio");
    assert_eq!(
        call_text(&router, &output_path).await,
        (StatusCode::OK, "{\"type\":\"thread.started\"}\n".into())
    );
    assert_eq!(
        call_text(&router, &format!("{path}/runs/{run_id}/output/stderr")).await,
        (StatusCode::OK, "warning on stderr\n".into())
    );
    assert_eq!(
        call(
            &router,
            "GET",
            &format!("{path}/runs/{run_id}/output/unknown"),
            serde_json::Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut update = input.clone();
    update["revision"] = 1.into();
    update["name"] = "Updated review".into();
    update["yolo"] = false.into();
    update["cleanup_worktree"] = false.into();
    assert_eq!(
        call(&router, "PUT", &path, update.clone()).await.0,
        StatusCode::OK
    );
    assert!(!manager.store.get_task(id).unwrap().input.yolo);
    assert!(manager.store.get_task(id).unwrap().input.cleanup_worktree);
    assert_eq!(
        call(&router, "PUT", &path, update).await.0,
        StatusCode::BAD_REQUEST
    );
    fs::write(&fake, "#!/bin/sh\necho manager-unavailable >&2\nexit 1\n").unwrap();
    let (code, paused) = call(
        &router,
        "PUT",
        &format!("{path}/enabled"),
        serde_json::json!({"enabled":false}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(paused["enabled"], false);
    assert!(
        paused["scheduler_error"]
            .as_str()
            .unwrap()
            .contains("manager-unavailable")
    );
    assert!(!manager.store.get_task(id).unwrap().input.enabled);
    fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
    let (_, synced) = call(
        &router,
        "POST",
        &format!("{path}/sync"),
        serde_json::Value::Null,
    )
    .await;
    assert!(synced["scheduler_error"].is_null());
    assert_eq!(
        call(
            &router,
            "POST",
            &format!("{path}/run"),
            serde_json::Value::Null
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        call(&router, "DELETE", &path, serde_json::Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(manager.store.get_task(id).unwrap().deleted);
    assert!(manager.store.run_path(id, &run_id).unwrap().exists());
    assert!(!manager.store.root.join("locks").exists());
    assert_eq!(
        call(
            &router,
            "GET",
            "/api/aow/automations",
            serde_json::Value::Null
        )
        .await
        .1,
        serde_json::json!([])
    );
    assert_eq!(
        call(
            &router,
            "GET",
            &format!("{path}/runs"),
            serde_json::Value::Null
        )
        .await
        .1[0]["session_id"],
        "native-session"
    );
    let mut invalid = input.clone();
    invalid["cron"] = "* * invalid".into();
    assert_eq!(
        call(&router, "POST", "/api/aow/automations", invalid)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut interval = input.clone();
    interval["cron"] = "".into();
    interval["interval_seconds"] = 90.into();
    interval["max_concurrent_runs"] = 3.into();
    let (code, interval_task) = call(&router, "POST", "/api/aow/automations", interval).await;
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(interval_task["interval_seconds"], 90);
    assert_eq!(interval_task["max_concurrent_runs"], 3);
    assert!(interval_task["next_run_at"].is_null());
    let persisted = manager
        .store
        .get_task(interval_task["id"].as_str().unwrap())
        .unwrap();
    assert_eq!(persisted.input.interval_seconds, Some(90));
    assert_eq!(persisted.input.max_concurrent_runs, 3);
    let mut invalid_concurrency = input.clone();
    invalid_concurrency["max_concurrent_runs"] = 0.into();
    assert_eq!(
        call(&router, "POST", "/api/aow/automations", invalid_concurrency)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut manual = input.clone();
    manual["kind"] = "manual".into();
    manual["cron"] = "".into();
    manual["prompt"] = "检查 {{分支}} {{不替换}}".into();
    manual["prompt_bindings"] =
        serde_json::json!([{"name":"分支", "placeholder":"{{分支}}", "start":7, "end":17}]);
    let (code, saved_manual) = call(&router, "POST", "/api/aow/automations", manual.clone()).await;
    assert_eq!(code, StatusCode::CREATED, "{saved_manual}");
    assert!(saved_manual["scheduler_error"].is_null());
    assert!(saved_manual["next_run_at"].is_null());
    assert_eq!(saved_manual["prompt_bindings"], manual["prompt_bindings"]);
    let manual_id = saved_manual["id"].as_str().unwrap();
    let manual_path = format!("/api/aow/automations/{manual_id}");
    assert!(
        !directory
            .path()
            .join("units")
            .join(format!("aow-automation-{manual_id}.timer"))
            .exists()
    );
    for body in [
        serde_json::json!({"revision":1}),
        serde_json::json!({"revision":1,"variables":{"分支":" "}}),
        serde_json::json!({"revision":0,"variables":{"分支":"main"}}),
        serde_json::json!({"revision":1,"variables":{"分支":"main","不替换":"extra"}}),
    ] {
        assert_eq!(
            call(&router, "POST", &format!("{manual_path}/run"), body)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (code, started) = call(
        &router,
        "POST",
        &format!("{manual_path}/run"),
        serde_json::json!({"revision":1,"variables":{"分支":"feature/login"}}),
    )
    .await;
    assert_eq!(code, StatusCode::ACCEPTED, "{started}");
    let pending = manager
        .store
        .take_manual_request(manual_id, started["run_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        pending
            .task
            .input
            .render_prompt(&pending.variables)
            .unwrap(),
        "检查 feature/login {{不替换}}"
    );
    let saved_run_id = started["run_id"].as_str().unwrap();
    let mut captured = initial_run(&pending.task, saved_run_id.into(), RunSource::Manual);
    captured.variables = Some(pending.variables.clone());
    drop(manager.store.create_run(&captured, &pending.task).unwrap());
    let (code, run_detail) = call(
        &router,
        "GET",
        &format!("{manual_path}/runs/{saved_run_id}"),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        run_detail["variables"],
        serde_json::json!({"分支":"feature/login"})
    );
    let (_, history) = call(
        &router,
        "GET",
        &format!("{manual_path}/runs"),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(history[0]["variables"], run_detail["variables"]);
    assert_eq!(
        call(&router, "DELETE", &manual_path, serde_json::Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    manual["prompt_bindings"][0]["start"] = 1.into();
    assert_eq!(
        call(&router, "POST", "/api/aow/automations", manual)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut invalid = input;
    invalid["workspace_path"] = directory.path().to_str().unwrap().into();
    assert_eq!(
        call(&router, "POST", "/api/aow/automations", invalid)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn api_accepts_temporary_workspace_tasks_without_a_base_branch() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repository");
    std::fs::create_dir(&repository).unwrap();
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
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repository)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let fake = directory.path().join("fake-agent");
    std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mut state = AppState::with_state_dir(
        directory.path().join("frontend"),
        directory.path().join("state"),
        directory.path().join("terminal.sock"),
    )
    .unwrap();
    state.auth = crate::auth::AuthService::disabled();
    state.automations.as_mut().unwrap().scheduler = Scheduler {
        platform: Platform::Systemd,
        runner: fake.clone(),
        directory: directory.path().join("units"),
        manager_command: fake.clone(),
        dispatch_command: fake.clone(),
    };
    let router = crate::build_router(state);
    let (code, project) = call(
        &router,
        "POST",
        "/api/aow/projects",
        serde_json::json!({"path":repository,"notes_path":directory.path().join("notes")}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{project}");
    let (code, _) = call(
        &router,
        "POST",
        "/api/aow/agents",
        serde_json::json!({"id":"codex","agent_type":"codex","display_name":"Codex test","command":fake}),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED);

    let (code, task) = call(
        &router,
        "POST",
        "/api/aow/automations",
        serde_json::json!({
            "name":"Temporary task", "prompt":"Inspect public status", "agent":"codex",
            "project_id":project["id"], "workspace_mode":"temporary", "workspace_path":repository,
            "base_branch":"", "cron":"0 9 * * *", "max_concurrent_runs":1, "enabled":true
        }),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{task}");
    assert_eq!(task["workspace_mode"], "temporary");
    assert_eq!(task["base_branch"], "");
}
