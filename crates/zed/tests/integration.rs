use anyhow::Result;
use aow_zed::{AcpHost, AcpService, Content, SessionImport, SessionSnapshot};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

struct Host {
    settings: RwLock<String>,
    execution_path: Option<std::ffi::OsString>,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(self.settings.read().unwrap().clone())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(self.execution_path.clone())
    }
    async fn read_text_file(&self, _cwd: &Path, path: &Path) -> Result<String> {
        Ok(tokio::fs::read_to_string(path).await?)
    }
    async fn write_text_file(&self, _cwd: &Path, path: &Path, content: &str) -> Result<()> {
        Ok(tokio::fs::write(path, content).await?)
    }
}
fn host(no_load: bool) -> Arc<Host> {
    Arc::new(Host { settings: RwLock::new(json!({ "agent_servers": { "test": { "type":"custom", "command":"node", "args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent.mjs")], "env":{"NO_LOAD":if no_load {"1"} else {"0"}}, "default_mode":"code", "default_config_options":{"enabled":true,"model":"large"} } } }).to_string()), execution_path: None })
}
fn prompt(value: &str) -> Vec<Content> {
    serde_json::from_value(json!([{ "type":"text", "text":value }])).unwrap()
}
async fn wait_for(
    service: &AcpService,
    id: &str,
    predicate: impl Fn(&SessionSnapshot) -> bool,
) -> SessionSnapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = service.snapshot(id).unwrap();
            if predicate(&snapshot) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "ACP state transition timed out: {:?}; logs: {:?}",
            service.snapshot(id),
            service.session_logs(id)
        )
    })
}

#[tokio::test]
async fn display_terminal_output_is_accumulated_and_persisted() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", workspace.clone()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.id, prompt("terminal"))?;
    let session = wait_for(&service, &session.id, |value| value.status == "idle").await;
    let terminal = &session.terminals["terminal-1"];
    assert_eq!(terminal["output"], "first line\nsecond line\n");
    assert_eq!(terminal["cwd"], workspace.to_string_lossy().as_ref());
    assert_eq!(terminal["exit_status"]["exitCode"], 0);
    let restored = AcpService::new(host(false), Some(state))?;
    assert_eq!(restored.snapshot(&session.id)?.terminals, session.terminals);
    Ok(())
}

#[tokio::test]
async fn lifecycle_defaults_events_permissions_files_cancel_and_reload() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", workspace.clone()).await?;
    let progress = service
        .connection_status("test", workspace.clone())
        .await?
        .unwrap();
    assert_eq!(progress.phase, "ready");
    assert!(!progress.running);
    let session = service.new_session(&connection.id).await?;
    assert!(session.error.is_none(), "{:?}", session.error);
    assert_eq!(session.modes["currentModeId"], "code");
    assert_eq!(session.config_options[0]["currentValue"], true);
    assert_eq!(session.config_options[1]["currentValue"], "large");
    service.start_prompt(&session.id, prompt("echo"))?;
    let session = wait_for(&service, &session.id, |value| value.status == "idle").await;
    assert_eq!(
        session
            .entries
            .iter()
            .filter(|entry| entry.kind == "user")
            .count(),
        1
    );
    assert_eq!(
        session
            .entries
            .iter()
            .find(|entry| entry.kind == "user")
            .unwrap()
            .content["text"],
        "echo"
    );
    assert_eq!(
        session
            .entries
            .iter()
            .find(|entry| entry.kind == "assistant")
            .unwrap()
            .content["text"],
        "Hello world"
    );
    assert_eq!(
        session
            .entries
            .iter()
            .find(|entry| entry.kind == "tool")
            .unwrap()
            .content["status"],
        "completed"
    );
    assert!(
        session
            .entries
            .iter()
            .any(|entry| entry.kind == "extension"
                && entry.content["privateData"]["preserve"] == true)
    );
    service.start_prompt(&session.id, prompt("permission"))?;
    let pending = wait_for(&service, &session.id, |value| !value.permissions.is_empty()).await;
    let preview = pending
        .entries
        .iter()
        .find(|entry| entry.id == "tool-1")
        .unwrap();
    assert_eq!(preview.content["title"], "Write file");
    assert_eq!(preview.content["content"][0]["newText"], "after");
    let request_id = &pending.permissions[0].id;
    assert!(
        service
            .answer(
                &session.id,
                request_id,
                json!({"outcome":{"outcome":"selected","optionId":"invented"}})
            )
            .is_err()
    );
    service.answer(
        &session.id,
        request_id,
        json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
    )?;
    wait_for(&service, &session.id, |value| value.status == "idle").await;
    std::fs::write(workspace.join("source.txt"), "first\nsecond\nthird\n")?;
    service.start_prompt(&session.id, prompt("files"))?;
    wait_for(&service, &session.id, |value| value.status == "idle").await;
    assert_eq!(
        std::fs::read_to_string(workspace.join("written.txt"))?,
        "second"
    );
    service.start_prompt(&session.id, prompt("cancel"))?;
    assert!(
        service
            .start_prompt(&session.id, prompt("must not run"))
            .is_err()
    );
    service.cancel(&session.id)?;
    let cancelled = wait_for(&service, &session.id, |value| value.status == "idle").await;
    assert_eq!(cancelled.stop_reason.as_deref(), Some("cancelled"));
    let logged = std::fs::read_to_string(workspace.join("prompts.log"))?;
    service.disconnect(&connection.id)?;
    wait_for(&service, &session.id, |value| {
        value.status == "disconnected"
    })
    .await;
    let resumed = service.resume(&session.id).await?;
    assert_eq!(resumed.id, session.id);
    assert_eq!(resumed.title, session.title);
    assert!(resumed.revision > cancelled.revision);
    assert_eq!(resumed.entries.len(), 2);
    assert_eq!(
        std::fs::read_to_string(workspace.join("prompts.log"))?,
        logged
    );
    assert!(
        service
            .session_logs(&session.id)?
            .iter()
            .any(|line| line["direction"] == "incoming")
    );
    let restored = AcpService::new(host(false), Some(state))?;
    assert_eq!(restored.snapshot(&session.id)?.status, "disconnected");
    assert!(restored.snapshot(&session.id)?.permissions.is_empty());
    Ok(())
}

#[tokio::test]
async fn activity_streams_and_dismissed_notices_survive_reload() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", workspace).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.id, prompt("activity"))?;
    let snapshot = wait_for(&service, &session.id, |value| value.status == "idle").await;
    assert_eq!(snapshot.entries.len(), 3);
    assert_eq!(snapshot.entries[1].content["text"], "**Hello**");
    assert_eq!(
        snapshot.entries[2].content["summary"][0]["text"],
        "**Summary**"
    );
    assert_eq!(snapshot.entries[2].content["status"], "completed");
    assert_eq!(snapshot.notices.len(), 1);
    let restored = AcpService::new(host(false), Some(state.clone()))?;
    assert_eq!(
        restored.snapshot(&session.id)?.notices[0].id,
        snapshot.notices[0].id
    );
    service.dismiss_notice(&session.id, &snapshot.notices[0].id)?;
    assert!(service.snapshot(&session.id)?.notices.is_empty());
    let restored = AcpService::new(host(false), Some(state))?;
    let snapshot = restored.snapshot(&session.id)?;
    assert!(snapshot.notices.is_empty());
    assert_eq!(snapshot.entries.len(), 3);
    assert_eq!(snapshot.plan["entries"][0]["status"], "completed");
    Ok(())
}

#[tokio::test]
async fn failed_load_preserves_history_and_the_live_event_route() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(false), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.id, prompt("first"))?;
    let before = wait_for(&service, &session.id, |value| value.status == "idle").await;
    std::fs::write(directory.path().join("fail-load"), "")?;
    assert!(
        service
            .load_session(&connection.id, &session.remote_id)
            .await
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(service.snapshot(&session.id)?.entries)?,
        serde_json::to_value(before.entries)?
    );
    service.start_prompt(&session.id, prompt("second"))?;
    let after = wait_for(&service, &session.id, |value| value.status == "idle").await;
    assert!(
        after
            .entries
            .iter()
            .filter(|entry| entry.kind == "assistant")
            .count()
            >= 2
    );
    Ok(())
}

#[tokio::test]
async fn composer_preferences_reuse_the_adapter_and_apply_to_new_sessions() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let host = host(false);
    let service = AcpService::new(host.clone(), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let mut settings: Value = serde_json::from_str(&host.settings.read().unwrap())?;
    settings["agent_servers"]["test"]["default_mode"] = "ask".into();
    settings["agent_servers"]["test"]["default_config_options"] =
        json!({"enabled":false,"model":"small"});
    settings["agent_servers"]["test"]["favorite_config_option_values"] = json!({"model":["small"]});
    *host.settings.write().unwrap() = settings.to_string();
    let reused = service.connect("test", directory.path().into()).await?;
    assert_eq!(connection.id, reused.id);
    let session = service.new_session(&reused.id).await?;
    assert_eq!(session.modes["currentModeId"], "ask");
    assert_eq!(session.config_options[0]["currentValue"], false);
    assert_eq!(session.config_options[1]["currentValue"], "small");
    Ok(())
}

#[tokio::test]
async fn changed_configuration_starts_a_fresh_adapter_and_resume_keeps_local_history() -> Result<()>
{
    let directory = tempfile::tempdir()?;
    let mut host = host(true);
    Arc::get_mut(&mut host).unwrap().execution_path = Some("/missing-host-path".into());
    let mut settings: Value = serde_json::from_str(&host.settings.read().unwrap())?;
    settings["agent_servers"]["test"]["env"]["PATH"] = std::env::var("PATH")?.into();
    settings["agent_servers"]["test"]["env"]["RESUME_ONLY"] = "1".into();
    *host.settings.write().unwrap() = settings.to_string();
    let service = AcpService::new(host.clone(), None)?;
    let first = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&first.id).await?;
    service.start_prompt(&session.id, prompt("original"))?;
    let before = wait_for(&service, &session.id, |value| value.status == "idle").await;
    settings["agent_servers"]["test"]["env"]["MARKER"] = "updated".into();
    *host.settings.write().unwrap() = settings.to_string();
    let second = service.connect("test", directory.path().into()).await?;
    assert_ne!(first.id, second.id);
    assert_eq!(second.agent_info["version"], "updated");
    assert_eq!(service.snapshot(&session.id)?.status, "idle");
    service.disconnect(&first.id)?;
    wait_for(&service, &session.id, |value| {
        value.status == "disconnected"
    })
    .await;
    let resumed = service.resume(&session.id).await?;
    assert_eq!(resumed.title, before.title);
    assert_eq!(
        serde_json::to_value(resumed.entries)?,
        serde_json::to_value(before.entries)?
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("prompts.log"))?,
        "original\n"
    );
    Ok(())
}

#[tokio::test]
async fn unsupported_load_never_replays_and_exit_drains_events() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(true), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.id, prompt("exit"))?;
    let exited = wait_for(&service, &session.id, |value| {
        value.status == "disconnected"
    })
    .await;
    assert!(
        exited
            .entries
            .iter()
            .any(|entry| entry.content["text"] == "last output")
    );
    assert!(
        service
            .resume(&session.id)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not support")
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("prompts.log"))?,
        "exit\n"
    );
    service.delete_session(&session.id).await?;
    assert!(service.snapshot(&session.id).is_err());
    Ok(())
}

#[tokio::test]
async fn request_scoped_authentication_remains_interactive() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(false), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    assert!(!session.auth_required);
    service.start_prompt(&session.id, prompt("auth-required"))?;
    let failed = wait_for(&service, &session.id, |value| value.auth_required).await;
    assert!(failed.error.as_deref().unwrap().contains("请重新登录"));
    let cloned = service.clone();
    let id = connection.id.clone();
    let login = tokio::spawn(async move { cloned.authenticate(&id, "login").await });
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut requests = service.connection_requests(&connection.id).unwrap();
            if let Some(request) = requests.pop() {
                return request;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    service.answer_connection(
        &connection.id,
        &request.id,
        json!({"action":"accept","content":{"code":"1234"}}),
    )?;
    login.await??;
    assert_eq!(service.connection_requests(&connection.id)?.len(), 0);
    let restored = service.snapshot(&session.id)?;
    assert!(!restored.auth_required);
    assert!(restored.error.is_none());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("prompts.log"))?,
        "auth-required\n"
    );
    Ok(())
}

#[tokio::test]
async fn empty_configuration_provider_is_distinct_from_legacy_modes() -> Result<()> {
    for (variable, supported) in [
        ("EMPTY_CONFIG", true),
        ("LEGACY_MODES", false),
        ("NULL_CONFIG", false),
    ] {
        let directory = tempfile::tempdir()?;
        let host = host(false);
        let mut settings: Value = serde_json::from_str(&host.settings.read().unwrap())?;
        settings["agent_servers"]["test"]["env"][variable] = "1".into();
        settings["agent_servers"]["test"]["default_config_options"] = json!({});
        *host.settings.write().unwrap() = settings.to_string();
        let service = AcpService::new(host, None)?;
        let connection = service.connect("test", directory.path().into()).await?;
        let session = service.new_session(&connection.id).await?;
        assert_eq!(session.config_options_supported, supported);
        assert_eq!(session.config_options, json!([]));
        assert!(
            !session.modes["availableModes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    Ok(())
}

#[test]
fn jsonc_settings_validate_known_fields_without_rewriting_extensions() {
    let source = "// comment\n{\"agent_servers\":{\"test\":{\"type\":\"custom\",\"command\":\"node\",\"future\":{\"x\":1},\"default_config_options\":{\"enabled\":true},\"favorite_config_option_values\":{\"model\":[\"a\"]},},},}";
    assert!(aow_zed::validate_settings(source).is_ok());
    for value in [
        Value::Null,
        json!([]),
        json!({"agent_servers":{"test":{"type":"extension"}}}),
        json!({"agent_servers":{"test":{"type":"custom","command":"node","default_config_options":{"x":1}}}}),
    ] {
        assert!(aow_zed::validate_settings(&value.to_string()).is_err());
    }
}

#[tokio::test]
#[ignore = "downloads a live Registry adapter; requires AOW_ACP_SMOKE_NODE and AOW_ACP_SMOKE_REGISTRY"]
async fn live_registry_adapter_initializes_with_node_only_path() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let launcher = directory.path().join("launcher");
    let profile = directory.path().join("profile");
    for path in [&state, &launcher, &profile] {
        std::fs::create_dir(path)?;
    }
    std::os::unix::fs::symlink(std::env::var("AOW_ACP_SMOKE_NODE")?, launcher.join("node"))?;
    std::fs::copy(
        std::env::var("AOW_ACP_SMOKE_REGISTRY")?,
        state.join("registry.json"),
    )?;
    let agent = std::env::var("AOW_ACP_SMOKE_AGENT").unwrap_or_else(|_| "codex-acp".into());
    let host = Arc::new(Host {
        settings: RwLock::new(
            json!({"agent_servers":{&agent:{"type":"registry","env":{
                "CODEX_HOME":profile, "CLAUDE_CONFIG_DIR":profile,
                "OPENAI_API_KEY":"", "CODEX_API_KEY":"", "ANTHROPIC_API_KEY":""
            }}}})
            .to_string(),
        ),
        execution_path: Some(std::env::join_paths([
            launcher,
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ])?),
    });
    let service = AcpService::new(host, Some(state))?;
    let connection = service.connect(&agent, directory.path().into()).await?;
    println!("Initialized {agent}: {}", connection.agent_info);
    assert!(connection.agent_info.is_object());
    match service.new_session(&connection.id).await {
        Ok(session) => {
            println!("Created {agent} session");
            service.close_session(&session.id).await?;
        }
        Err(error) => {
            let message = error.to_string();
            ensure_authentication_required(&message)?;
            println!(
                "{agent} initialized; a fresh profile requires authentication before session/new"
            );
        }
    }
    service.disconnect(&connection.id)?;
    Ok(())
}

fn ensure_authentication_required(message: &str) -> Result<()> {
    anyhow::ensure!(
        message.to_lowercase().contains("auth"),
        "Unexpected session error: {message}"
    );
    Ok(())
}

#[tokio::test]
async fn imported_metadata_persists_without_loading_until_opened() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", workspace.clone()).await?;
    let remote = service.remote_sessions(&connection.id, None).await?;
    let metadata = SessionImport {
        remote_id: remote["sessions"][0]["sessionId"].as_str().unwrap().into(),
        cwd: connection.cwd.clone(),
        title: Some("Imported title".into()),
        updated_at: Some("2020-01-01T08:00:00+08:00".into()),
    };
    assert!(
        service.sessions(None).is_empty(),
        "discovery cannot register history"
    );
    let before = service.logs(&connection.id)?;
    let imported =
        service.import_sessions(&connection.id, vec![metadata.clone(), metadata.clone()])?;
    assert_eq!(imported.len(), 1);
    assert_eq!(
        service.logs(&connection.id)?,
        before,
        "import must send no protocol requests"
    );
    let id = &imported[0].id;
    let snapshot = service.snapshot(id)?;
    assert!(snapshot.needs_load);
    assert!(snapshot.entries.is_empty());
    assert_eq!(snapshot.updated_at, "2020-01-01T00:00:00+00:00");
    assert_eq!(snapshot.status, "disconnected");
    assert!(service.sessions(Some("/other-workspace")).is_empty());
    service.disconnect(&connection.id)?;
    let restored = AcpService::new(host(false), Some(state))?;
    assert_eq!(
        serde_json::to_value(restored.snapshot(id)?)?,
        serde_json::to_value(snapshot)?
    );
    let loaded = restored.resume(id).await?;
    assert!(!loaded.needs_load);
    assert_eq!(loaded.title, "Imported title");
    assert!(
        loaded
            .entries
            .iter()
            .any(|entry| entry.content["text"] == "previous answer")
    );
    let connection = restored.connect("test", workspace).await?;
    restored.start_prompt(id, prompt("cancel"))?;
    let working = restored.snapshot(id)?;
    assert_eq!(working.status, "working");
    assert!(
        restored
            .import_sessions(&connection.id, vec![metadata])?
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(restored.snapshot(id)?)?,
        serde_json::to_value(working)?
    );
    restored.cancel(id)?;
    Ok(())
}

#[tokio::test]
async fn import_validates_workspace_and_ids_before_saving_and_can_retry_load() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(false), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let metadata = SessionImport {
        remote_id: "session-1".into(),
        cwd: connection.cwd.clone(),
        title: None,
        updated_at: None,
    };
    for invalid in [
        SessionImport {
            cwd: "/other".into(),
            ..metadata.clone()
        },
        SessionImport {
            remote_id: " ".into(),
            ..metadata.clone()
        },
    ] {
        assert!(
            service
                .import_sessions(&connection.id, vec![metadata.clone(), invalid])
                .is_err()
        );
        assert!(service.sessions(None).is_empty());
    }
    let imported = service.import_sessions(&connection.id, vec![metadata])?;
    let id = &imported[0].id;
    std::fs::write(directory.path().join("fail-load"), "")?;
    assert!(service.resume(id).await.is_err());
    assert!(service.snapshot(id)?.needs_load);
    assert!(service.snapshot(id)?.entries.is_empty());
    std::fs::remove_file(directory.path().join("fail-load"))?;
    let loaded = service.resume(id).await?;
    assert!(!loaded.needs_load);
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(service.sessions(None).len(), 1);
    Ok(())
}
