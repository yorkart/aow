use anyhow::Result;
use aow_acp::{AcpHost, AcpService, Content, SessionImport, SessionSnapshot};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

struct Host {
    completions: std::sync::Mutex<Vec<aow_acp::TurnCompletion>>,
    settings: RwLock<String>,
    execution_path: Option<std::ffi::OsString>,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn turn_completed(&self, completion: aow_acp::TurnCompletion) {
        self.completions.lock().unwrap().push(completion);
    }
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
    Arc::new(Host { completions: Default::default(), settings: RwLock::new(json!({ "agent_servers": { "test": { "type":"custom", "command":"node", "args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent.mjs")], "env":{"NO_LOAD":if no_load {"1"} else {"0"}}, "default_mode":"code", "default_config_options":{"enabled":true,"model":"large"} } } }).to_string()), execution_path: None })
}
fn prompt(value: &str) -> Vec<Content> {
    serde_json::from_value(json!([{ "type":"text", "text":value }])).unwrap()
}

#[tokio::test]
async fn unsupported_protocol_versions_fail_before_session_creation_and_allow_retry() -> Result<()>
{
    for version in [0, 2, u16::MAX] {
        let directory = tempfile::tempdir()?;
        let host = host(false);
        let mut settings: Value = serde_json::from_str(&host.settings.read().unwrap())?;
        settings["agent_servers"]["test"]["env"]["PROTOCOL_VERSION"] = version.to_string().into();
        *host.settings.write().unwrap() = settings.to_string();
        let service = AcpService::new(host.clone(), None)?;
        let error = service
            .connect("test", directory.path().into())
            .await
            .unwrap_err();
        // The SDK can reject negotiation before our response-version guard is reached.
        assert!(error.to_string().contains("protocol version"), "{error:#}");
        assert!(service.sessions(None).is_empty());

        settings["agent_servers"]["test"]["env"]["PROTOCOL_VERSION"] = "1".into();
        *host.settings.write().unwrap() = settings.to_string();
        let connection = service.connect("test", directory.path().into()).await?;
        let session = service.new_session(&connection.id).await?;
        service.start_prompt(&session.thread_id, prompt("retry"))?;
        let snapshot = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
        assert_eq!(snapshot.stop_reason.as_deref(), Some("end_turn"));
        service.disconnect(&connection.id)?;
    }
    Ok(())
}

#[tokio::test]
async fn sidebar_recency_changes_only_when_a_prompt_is_submitted() -> Result<()> {
    for resume_only in [false, true] {
        let directory = tempfile::tempdir()?;
        let state = directory.path().join("state");
        let host = host(resume_only);
        if resume_only {
            let mut settings: Value = serde_json::from_str(&host.settings.read().unwrap())?;
            settings["agent_servers"]["test"]["env"]["RESUME_ONLY"] = "1".into();
            *host.settings.write().unwrap() = settings.to_string();
        }
        let service = AcpService::new(host.clone(), Some(state.clone()))?;
        let connection = service.connect("test", directory.path().into()).await?;
        let imported = service.import_sessions(
            &connection.id,
            ["2020-01-01T00:00:00+00:00", "2021-01-01T00:00:00+00:00"]
                .into_iter()
                .enumerate()
                .map(|(index, time)| SessionImport {
                    session_id: format!("session-{}", index + 1),
                    cwd: connection.cwd.clone(),
                    title: Some(format!("Imported {index}")),
                    updated_at: Some(time.into()),
                })
                .collect(),
        )?;
        let id = &imported[0].thread_id;
        let baseline = imported[0].interacted_at.clone();
        assert_eq!(baseline.as_deref(), Some("2020-01-01T00:00:00+00:00"));
        let loaded = service.resume(id).await?;
        assert_eq!(loaded.interacted_at, baseline);
        assert_eq!(service.overview(id)?.interacted_at, baseline);
        assert_ne!(loaded.updated_at, imported[0].updated_at);
        assert_eq!(service.sessions(None)[0].thread_id, imported[1].thread_id);
        service.set_mode(id, "ask").await?;
        service
            .set_config_option(id, "enabled", json!(false))
            .await?;
        assert_eq!(service.snapshot(id)?.interacted_at, baseline);

        service.start_prompt(id, prompt("checkpoint"))?;
        let submitted = service.snapshot(id)?;
        assert_ne!(submitted.interacted_at, baseline);
        assert_eq!(service.sessions(None)[0].thread_id, *id);
        let streamed = wait_for(&service, id, |snapshot| {
            snapshot
                .entries
                .iter()
                .any(|entry| entry.content["text"] == "Unfinished streamed response")
        })
        .await;
        assert_eq!(streamed.interacted_at, submitted.interacted_at);
        assert!(streamed.revision > submitted.revision);
        service.cancel(id)?;
        let completed = wait_for(&service, id, |snapshot| snapshot.status == "idle").await;
        assert_eq!(completed.interacted_at, submitted.interacted_at);
        service.disconnect(&connection.id)?;
        service.flush_threads_on_quit()?;
        drop(service);

        let restored = AcpService::new(host, Some(state))?;
        assert_eq!(restored.sessions(None)[0].thread_id, *id);
        assert_eq!(
            restored.session_info(id)?.interacted_at,
            submitted.interacted_at
        );
        assert_eq!(
            restored.snapshot(id)?.interacted_at,
            submitted.interacted_at
        );
        let resumed = restored.resume(id).await?;
        assert_eq!(resumed.interacted_at, submitted.interacted_at);
        assert_eq!(
            restored.overview(id)?.interacted_at,
            submitted.interacted_at
        );
    }
    Ok(())
}

#[tokio::test]
async fn completion_usage_omits_restored_missing_or_reset_baselines() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let host = host(false);
    let service = AcpService::new(host.clone(), Some(state.clone()))?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.thread_id, prompt("notify-usage:100"))?;
    wait_for(&service, &session.thread_id, |s| s.status == "idle").await;
    service.flush_threads_on_quit()?;
    let restored = AcpService::new(host.clone(), Some(state))?;
    restored.resume(&session.thread_id).await?;
    for value in [
        "notify-usage:300",
        "notify-usage:20",
        "message-ids",
        "notify-usage:100",
        "notify-usage:200",
    ] {
        restored.start_prompt(&session.thread_id, prompt(value))?;
        wait_for(&restored, &session.thread_id, |s| s.status == "idle").await;
    }
    let events = host.completions.lock().unwrap();
    assert_eq!(events.len(), 6);
    for index in [1, 2, 3, 4] {
        assert!(
            events[index].usage.is_none(),
            "unknown or reset usage must not be labeled as turn usage"
        );
    }
    assert_eq!(events[5].usage.as_ref().unwrap()["totalTokens"], 150);
    Ok(())
}

#[tokio::test]
async fn completion_callbacks_are_per_turn_and_never_replayed_from_history() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let host = host(false);
    let service = AcpService::new(host.clone(), Some(state.clone()))?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    for total in [100, 300] {
        service.start_prompt(&session.thread_id, prompt(&format!("notify-usage:{total}")))?;
        wait_for(&service, &session.thread_id, |s| s.status == "idle").await;
    }
    {
        let events = host.completions.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_ne!(events[0].turn_id, events[1].turn_id);
        assert_eq!(events[1].session.thread_id, session.thread_id);
        assert_eq!(events[0].conclusion.as_deref(), Some("Turn 100"));
        assert_eq!(events[1].conclusion.as_deref(), Some("Turn 300"));
        assert_eq!(events[0].usage.as_ref().unwrap()["totalTokens"], 150);
        assert_eq!(events[1].usage.as_ref().unwrap()["totalTokens"], 300);
    }
    for value in ["cancel", "notify-limit", "notify-error", "auth-required"] {
        service.start_prompt(&session.thread_id, prompt(value))?;
        if value == "cancel" {
            service.cancel(&session.thread_id)?;
        }
        wait_for(&service, &session.thread_id, |s| s.status == "idle").await;
        assert_eq!(
            host.completions.lock().unwrap().len(),
            2,
            "{value} is not a successful completion"
        );
    }
    service.start_prompt(&session.thread_id, prompt("permission"))?;
    let waiting = wait_for(&service, &session.thread_id, |s| !s.permissions.is_empty()).await;
    assert_eq!(
        host.completions.lock().unwrap().len(),
        2,
        "awaiting permission is not completion"
    );
    service.answer(
        &session.thread_id,
        &waiting.permissions[0].id,
        json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
    )?;
    wait_for(&service, &session.thread_id, |s| s.status == "idle").await;
    assert_eq!(host.completions.lock().unwrap().len(), 3);
    assert!(host.completions.lock().unwrap()[2].usage.is_none());
    service.flush_threads_on_quit()?;
    let restored = AcpService::new(host.clone(), Some(state))?;
    for _ in 0..3 {
        restored.snapshot(&session.thread_id)?;
    }
    restored.resume(&session.thread_id).await?;
    assert_eq!(
        host.completions.lock().unwrap().len(),
        3,
        "loading history never emits completion"
    );
    restored.start_prompt(&session.thread_id, prompt("message-ids"))?;
    wait_for(&restored, &session.thread_id, |s| s.status == "idle").await;
    let events = host.completions.lock().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(
        events[3].conclusion.as_deref(),
        Some("ACP_LIVE_FOLLOWUP_VISIBLE")
    );
    Ok(())
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
async fn protocol_message_boundaries_survive_transport_and_reload() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", workspace).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.thread_id, prompt("message-ids"))?;
    let completed = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    assert_eq!(completed.entries.len(), 3);
    assert_eq!(completed.entries[1].content["text"], "ACP_LIVE_MAIN_DONE");
    assert_eq!(
        completed.entries[2].content["text"],
        "ACP_LIVE_FOLLOWUP_VISIBLE"
    );
    assert_eq!(completed.entries[1].message_id.as_deref(), Some("msg_119"));
    assert_eq!(completed.entries[2].message_id.as_deref(), Some("msg_122"));
    assert_eq!(completed.stop_reason.as_deref(), Some("end_turn"));
    service.flush_threads_on_quit()?;
    let restored = AcpService::new(host(false), Some(state))?;
    assert_eq!(
        serde_json::to_value(restored.snapshot(&session.thread_id)?.entries)?,
        serde_json::to_value(completed.entries)?
    );
    Ok(())
}

#[tokio::test]
async fn image_prompts_round_trip_without_duplicate_echo_and_survive_reload() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", workspace.clone()).await?;
    let session = service.new_session(&connection.id).await?;
    let image = json!({"type":"image", "mimeType":"image/png", "data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII="});
    let content = json!([
        {"type":"text", "text":"Compare "}, image.clone(),
        {"type":"text", "text":" with "}, image.clone()
    ]);
    service.start_prompt(&session.thread_id, serde_json::from_value(content.clone())?)?;
    let completed = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    let user_content = |snapshot: &SessionSnapshot| {
        snapshot
            .entries
            .iter()
            .filter(|entry| entry.kind == "user")
            .map(|entry| entry.content.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(serde_json::to_value(user_content(&completed))?, content);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(workspace.join("image-prompt.json"))?)?,
        content
    );
    // AoW snapshots retain the original Base64, independently of a running adapter.
    let restored = AcpService::new(host(false), Some(state))?;
    assert_eq!(
        user_content(&restored.snapshot(&session.thread_id)?),
        user_content(&completed)
    );
    let loaded = restored.resume(&session.thread_id).await?;
    assert_eq!(user_content(&loaded), user_content(&completed));
    // Pure image messages are valid prompts too.
    restored.start_prompt(&session.thread_id, serde_json::from_value(json!([image]))?)?;
    wait_for(&restored, &session.thread_id, |value| {
        value.status == "idle"
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn images_require_the_advertised_prompt_capability() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let host = host(false);
    let mut settings: Value = serde_json::from_str(&host.settings.read().unwrap())?;
    settings["agent_servers"]["test"]["env"]["NO_IMAGES"] = "1".into();
    *host.settings.write().unwrap() = settings.to_string();
    let service = AcpService::new(host, Some(directory.path().join("state")))?;
    let connection = service
        .connect("test", directory.path().canonicalize()?)
        .await?;
    let session = service.new_session(&connection.id).await?;
    let content =
        serde_json::from_value(json!([{"type":"image", "mimeType":"image/png", "data":"AA=="}]))?;
    assert!(
        service
            .start_prompt(&session.thread_id, content)
            .unwrap_err()
            .to_string()
            .contains("does not support images")
    );
    assert!(service.snapshot(&session.thread_id)?.entries.is_empty());
    assert!(!directory.path().join("image-prompt.json").exists());
    Ok(())
}

#[tokio::test]
async fn display_terminal_output_is_accumulated_and_persisted() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", workspace.clone()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.thread_id, prompt("terminal"))?;
    let session = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    let terminal = &session.terminals["terminal-1"];
    assert_eq!(terminal["output"], "first line\nsecond line\n");
    assert_eq!(terminal["cwd"], workspace.to_string_lossy().as_ref());
    assert_eq!(terminal["exit_status"]["exitCode"], 0);
    let overview = service.overview(&session.thread_id)?;
    assert!(overview.entries.is_empty() && overview.terminals.is_empty());
    let turn = overview.overview.as_ref().unwrap().last().unwrap();
    assert_eq!(turn.tool_count, 1);
    assert_eq!(turn.conclusion[0].content["text"], "Command completed");
    let aow_acp::SessionUpdates::Snapshot { snapshot: details } =
        service.process(&session.thread_id, &turn.id, None, None)?
    else {
        panic!("initial process response")
    };
    assert_eq!(details.terminals["terminal-1"], *terminal);
    let restored = AcpService::new(host(false), Some(state))?;
    assert_eq!(
        restored.snapshot(&session.thread_id)?.terminals,
        session.terminals
    );
    let restored_overview = restored.overview(&session.thread_id)?;
    assert!(restored_overview.entries.is_empty() && restored_overview.terminals.is_empty());
    assert_ne!(restored_overview.sync_generation, overview.sync_generation);
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
    service.start_prompt(&session.thread_id, prompt("echo"))?;
    let session = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
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
    service.start_prompt(&session.thread_id, prompt("permission"))?;
    let pending = wait_for(&service, &session.thread_id, |value| {
        !value.permissions.is_empty()
    })
    .await;
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
                &session.thread_id,
                request_id,
                json!({"outcome":{"outcome":"selected","optionId":"invented"}})
            )
            .is_err()
    );
    service.answer(
        &session.thread_id,
        request_id,
        json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
    )?;
    wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    std::fs::write(workspace.join("source.txt"), "first\nsecond\nthird\n")?;
    service.start_prompt(&session.thread_id, prompt("files"))?;
    wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    assert_eq!(
        std::fs::read_to_string(workspace.join("written.txt"))?,
        "second"
    );
    service.start_prompt(&session.thread_id, prompt("cancel"))?;
    assert!(
        service
            .start_prompt(&session.thread_id, prompt("must not run"))
            .is_err()
    );
    service.cancel(&session.thread_id)?;
    let cancelled = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    assert_eq!(cancelled.stop_reason.as_deref(), Some("cancelled"));
    let logged = std::fs::read_to_string(workspace.join("prompts.log"))?;
    service.disconnect(&connection.id)?;
    wait_for(&service, &session.thread_id, |value| {
        value.status == "disconnected"
    })
    .await;
    let resumed = service.resume(&session.thread_id).await?;
    assert_eq!(resumed.thread_id, session.thread_id);
    assert_eq!(resumed.title, session.title);
    assert!(resumed.revision > cancelled.revision);
    assert_eq!(resumed.entries.len(), 2);
    assert_eq!(
        std::fs::read_to_string(workspace.join("prompts.log"))?,
        logged
    );
    assert!(
        service
            .session_logs(&session.thread_id)?
            .iter()
            .any(|line| line["direction"] == "incoming")
    );
    let restored = AcpService::new(host(false), Some(state))?;
    assert_eq!(
        restored.snapshot(&session.thread_id)?.status,
        "disconnected"
    );
    assert!(
        restored
            .snapshot(&session.thread_id)?
            .permissions
            .is_empty()
    );
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
    service.start_prompt(&session.thread_id, prompt("activity"))?;
    let snapshot = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
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
        restored.snapshot(&session.thread_id)?.notices[0].id,
        snapshot.notices[0].id
    );
    service.dismiss_notice(&session.thread_id, &snapshot.notices[0].id)?;
    assert!(service.snapshot(&session.thread_id)?.notices.is_empty());
    let restored = AcpService::new(host(false), Some(state.clone()))?;
    let snapshot = restored.snapshot(&session.thread_id)?;
    assert!(snapshot.notices.is_empty());
    assert_eq!(snapshot.entries.len(), 3);
    assert_eq!(
        snapshot.entries[2].content["summary"][0]["text"],
        "**Summary**"
    );
    assert_eq!(snapshot.entries[2].content["status"], "completed");
    assert_eq!(snapshot.plan["entries"][0]["status"], "completed");
    service.clear_plan(&session.thread_id)?;
    let cleared = service.snapshot(&session.thread_id)?;
    assert!(cleared.plan.is_null());
    assert!(cleared.revision > snapshot.revision);
    assert_eq!(cleared.entries.len(), snapshot.entries.len());
    service.flush_threads_on_quit()?;
    let restored = AcpService::new(host(false), Some(state))?;
    assert!(restored.snapshot(&session.thread_id)?.plan.is_null());
    service.start_prompt(&session.thread_id, prompt("activity"))?;
    let next = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    assert_eq!(next.plan["entries"][0]["status"], "completed");
    Ok(())
}

#[tokio::test]
async fn plan_is_retained_after_completion_and_cleared_when_the_next_turn_starts() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(false), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.thread_id, prompt("activity"))?;
    let completed = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    assert_eq!(completed.plan["entries"][0]["status"], "completed");

    // A rejected send must not erase the plan of the completed turn.
    assert!(service.start_prompt(&session.thread_id, vec![]).is_err());
    assert_eq!(service.snapshot(&session.thread_id)?.plan, completed.plan);

    // This prompt stays running and emits no replacement plan.
    service.start_prompt(&session.thread_id, prompt("checkpoint"))?;
    let working = service.snapshot(&session.thread_id)?;
    assert_eq!(working.status, "working");
    assert!(working.plan.is_null());
    assert!(service.overview(&session.thread_id)?.plan.is_null());
    assert!(working.revision > completed.revision);
    service.cancel(&session.thread_id)?;
    let cancelled = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    assert!(cancelled.plan.is_null());

    // New plan updates must still appear and remain after the new turn finishes.
    service.start_prompt(&session.thread_id, prompt("activity"))?;
    let next = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    assert_eq!(next.plan["entries"][0]["status"], "completed");
    service.disconnect(&connection.id)?;
    Ok(())
}

#[tokio::test]
async fn failed_load_preserves_history_and_the_live_event_route() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(false), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.thread_id, prompt("first"))?;
    let before = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    std::fs::write(directory.path().join("fail-load"), "")?;
    assert!(
        service
            .load_session(&connection.id, session.session_id.as_deref().unwrap())
            .await
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(service.snapshot(&session.thread_id)?.entries)?,
        serde_json::to_value(before.entries)?
    );
    service.start_prompt(&session.thread_id, prompt("second"))?;
    let after = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
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
    service.start_prompt(&session.thread_id, prompt("original"))?;
    let before = wait_for(&service, &session.thread_id, |value| value.status == "idle").await;
    settings["agent_servers"]["test"]["env"]["MARKER"] = "updated".into();
    *host.settings.write().unwrap() = settings.to_string();
    let second = service.connect("test", directory.path().into()).await?;
    assert_ne!(first.id, second.id);
    assert_eq!(second.agent_info["version"], "updated");
    assert_eq!(service.snapshot(&session.thread_id)?.status, "idle");
    service.disconnect(&first.id)?;
    wait_for(&service, &session.thread_id, |value| {
        value.status == "disconnected"
    })
    .await;
    let resumed = service.resume(&session.thread_id).await?;
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
    service.start_prompt(&session.thread_id, prompt("exit"))?;
    let exited = wait_for(&service, &session.thread_id, |value| {
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
            .resume(&session.thread_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not support")
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("prompts.log"))?,
        "exit\n"
    );
    service.delete_session(&session.thread_id).await?;
    assert!(service.snapshot(&session.thread_id).is_err());
    Ok(())
}

#[tokio::test]
async fn request_scoped_authentication_remains_interactive() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(false), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    assert!(!session.auth_required);
    service.start_prompt(&session.thread_id, prompt("auth-required"))?;
    let failed = wait_for(&service, &session.thread_id, |value| value.auth_required).await;
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
    let restored = service.snapshot(&session.thread_id)?;
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
    assert!(aow_acp::validate_settings(source).is_ok());
    for value in [
        Value::Null,
        json!([]),
        json!({"agent_servers":{"test":{"type":"extension"}}}),
        json!({"agent_servers":{"test":{"type":"custom","command":"node","default_config_options":{"x":1}}}}),
    ] {
        assert!(aow_acp::validate_settings(&value.to_string()).is_err());
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
        completions: Default::default(),
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
            service.close_session(&session.thread_id).await?;
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
        session_id: remote["sessions"][0]["sessionId"].as_str().unwrap().into(),
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
    let id = &imported[0].thread_id;
    let snapshot = service.snapshot(id)?;
    assert!(snapshot.needs_load);
    assert!(snapshot.entries.is_empty());
    assert_eq!(snapshot.updated_at, "2020-01-01T00:00:00+00:00");
    assert_eq!(snapshot.status, "disconnected");
    assert!(service.sessions(Some("/other-workspace")).is_empty());
    service.disconnect(&connection.id)?;
    let restored = AcpService::new(host(false), Some(state))?;
    let mut restored_snapshot = restored.snapshot(id)?;
    assert_ne!(restored_snapshot.sync_generation, snapshot.sync_generation);
    assert!(matches!(
        restored.updates(id, &snapshot.sync_generation, snapshot.revision)?,
        aow_acp::SessionUpdates::Snapshot { .. }
    ));
    // The cursor incarnation changes; the persisted conversation does not.
    restored_snapshot.sync_generation = snapshot.sync_generation.clone();
    assert_eq!(
        serde_json::to_value(restored_snapshot)?,
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
        session_id: "session-1".into(),
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
            session_id: " ".into(),
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
    let id = &imported[0].thread_id;
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

#[tokio::test]
async fn shutdown_flush_commits_streamed_content_without_waiting_for_prompt_completion()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(&session.thread_id, prompt("checkpoint"))?;
    wait_for(&service, &session.thread_id, |snapshot| {
        snapshot
            .entries
            .iter()
            .any(|entry| entry.content["text"] == "Unfinished streamed response")
    })
    .await;
    service.flush_threads_on_quit()?;
    let restored = AcpService::new(host(false), Some(state.clone()))?;
    let snapshot = restored.snapshot(&session.thread_id)?;
    assert_eq!(snapshot.status, "disconnected");
    assert!(
        snapshot
            .entries
            .iter()
            .any(|entry| entry.content["text"] == "Unfinished streamed response")
    );
    assert!(state.join("threads.sqlite").is_file());
    assert!(!state.join("history").exists());
    service.cancel(&session.thread_id)?;
    Ok(())
}

#[tokio::test]
async fn failed_prompt_persistence_does_not_send_or_leave_the_session_working() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let service = AcpService::new(host(false), Some(state.clone()))?;
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    let database = rusqlite::Connection::open(state.join("threads.sqlite"))?;
    database.execute_batch(include_str!("sql/reject_snapshot_writes.sql"))?;
    assert!(
        service
            .start_prompt(&session.thread_id, prompt("echo"))
            .is_err()
    );
    let failed = service.snapshot(&session.thread_id)?;
    assert_eq!(failed.status, "idle");
    assert_eq!(failed.interacted_at, session.interacted_at);
    assert!(failed.entries.is_empty());
    assert!(
        failed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("test disk failure"))
    );
    assert!(!directory.path().join("prompts.log").exists());
    database.execute_batch(include_str!("sql/allow_snapshot_writes.sql"))?;
    service.flush_threads_on_quit()?;
    service.start_prompt(&session.thread_id, prompt("echo"))?;
    let finished = wait_for(&service, &session.thread_id, |snapshot| {
        snapshot.status == "idle"
    })
    .await;
    assert_eq!(
        finished
            .entries
            .iter()
            .filter(|entry| entry.kind == "user")
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("prompts.log"))?,
        "echo\n"
    );
    Ok(())
}

fn correlation_host() -> Arc<Host> {
    Arc::new(Host { completions: Default::default(),
        settings: RwLock::new(json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/correlation-agent.mjs")]}}}).to_string()),
        execution_path: None,
    })
}

#[tokio::test]
async fn thread_identity_is_durable_before_initialization_and_survives_creation_failures()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let state = workspace.join("state");
    let service = AcpService::new(correlation_host(), Some(state.clone()))?;
    let prepared = service.prepare_session("test", workspace.clone()).await?;
    assert!(prepared.session_id.is_none());
    assert!(!workspace.join("initializations.log").exists());
    assert_eq!(
        service.session_info(&prepared.thread_id)?.cwd,
        workspace.to_string_lossy()
    );
    assert!(service.session_logs(&prepared.thread_id)?.is_empty());
    let db = rusqlite::Connection::open(state.join("threads.sqlite"))?;
    let persisted: Option<String> = db.query_row(
        include_str!("sql/session_id.sql"),
        [&prepared.thread_id],
        |row| row.get(0),
    )?;
    assert!(persisted.is_none());
    let service = AcpService::new(correlation_host(), Some(state))?;
    tokio::fs::write(workspace.join("fail-initialize"), "").await?;
    assert!(service.resume(&prepared.thread_id).await.is_err());
    assert!(
        service
            .session_logs(&prepared.thread_id)?
            .iter()
            .any(|entry| entry["message"]
                .as_str()
                .unwrap()
                .contains("Initialization failed"))
    );
    assert!(service.snapshot(&prepared.thread_id)?.error.is_none());
    assert!(
        service
            .snapshot(&prepared.thread_id)?
            .recovery
            .unwrap()
            .error
            .is_some()
    );
    let failed_logs = service.session_logs(&prepared.thread_id)?;
    let failed_connection = failed_logs[0]["connection_id"].as_str().unwrap();
    assert_eq!(
        service.logs(failed_connection)?,
        failed_logs,
        "failed initialization retains its connection log without a live connection"
    );
    tokio::fs::remove_file(workspace.join("fail-initialize")).await?;
    tokio::fs::write(workspace.join("fail-new"), "").await?;
    let error = service.resume(&prepared.thread_id).await.unwrap_err();
    assert!(aow_acp::is_auth_required(&error));
    let failed = service.snapshot(&prepared.thread_id)?;
    assert!(failed.auth_required);
    assert!(failed.session_id.is_none());
    assert!(
        service
            .session_logs(&prepared.thread_id)?
            .iter()
            .any(|entry| entry["thread_id"] == prepared.thread_id
                && entry["message"]
                    .as_str()
                    .unwrap()
                    .contains("Authenticate before creation"))
    );
    tokio::fs::remove_file(workspace.join("fail-new")).await?;
    let created = service.resume(&prepared.thread_id).await?;
    assert_eq!(created.thread_id, prepared.thread_id);
    assert!(created.session_id.is_some());
    assert!(!created.auth_required);
    assert_eq!(service.sessions(None).len(), 1);
    let json = serde_json::to_value(&created)?;
    assert!(json.get("id").is_none() && json.get("remote_id").is_none());
    assert_eq!(json["thread_id"], prepared.thread_id);
    assert!(
        service
            .session_logs(&prepared.thread_id)?
            .iter()
            .map(|entry| entry["connection_id"].clone())
            .collect::<Vec<_>>()
            .len()
            > 2
    );
    Ok(())
}

#[tokio::test]
async fn reused_connection_logs_isolate_concurrent_threads_and_follow_recovery() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let service = AcpService::new(correlation_host(), Some(workspace.join("state")))?;
    let first = service.prepare_session("test", workspace.clone()).await?;
    let second = service.prepare_session("test", workspace.clone()).await?;
    let (first, second) = tokio::try_join!(
        service.resume(&first.thread_id),
        service.resume(&second.thread_id)
    )?;
    assert_ne!(first.session_id, second.session_id);
    let connection = service.connect("test", workspace.clone()).await?;
    assert_eq!(
        tokio::fs::read_to_string(workspace.join("initializations.log"))
            .await?
            .lines()
            .count(),
        1
    );
    service.start_prompt(&first.thread_id, prompt("ONLY-FIRST"))?;
    service.start_prompt(&second.thread_id, prompt("ONLY-SECOND"))?;
    for thread in [&second, &first] {
        let pending = wait_for(&service, &thread.thread_id, |snapshot| {
            !snapshot.permissions.is_empty()
        })
        .await;
        service.answer(
            &thread.thread_id,
            &pending.permissions[0].id,
            json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
        )?;
    }
    for (thread, other, own_text, other_text) in [
        (&first, &second, "ONLY-FIRST", "ONLY-SECOND"),
        (&second, &first, "ONLY-SECOND", "ONLY-FIRST"),
    ] {
        wait_for(&service, &thread.thread_id, |snapshot| {
            snapshot.status == "idle"
        })
        .await;
        let logs = service.session_logs(&thread.thread_id)?;
        assert!(logs.iter().any(|entry| entry["thread_id"].is_null()
            && entry["scope"] == "connection"
            && entry["message"].as_str().unwrap().contains("initialize")));
        assert!(
            logs.iter()
                .any(|entry| entry["message"].as_str().unwrap().contains(own_text))
        );
        assert!(
            logs.iter()
                .all(|entry| entry["thread_id"] != other.thread_id
                    && !entry["message"].as_str().unwrap().contains(other_text))
        );
        assert!(
            logs.iter()
                .any(|entry| entry["thread_id"] == thread.thread_id
                    && entry["message"].as_str().unwrap().contains("session/new"))
        );
        assert!(logs.iter().any(|entry| entry["direction"] == "outgoing"
            && entry["message"].as_str().unwrap().contains("selected")));
    }
    service.disconnect(&connection.id)?;
    wait_for(&service, &first.thread_id, |snapshot| {
        snapshot.status == "disconnected"
    })
    .await;
    service.resume(&first.thread_id).await?;
    let logs = service.session_logs(&first.thread_id)?;
    let connection_ids: std::collections::HashSet<_> = logs
        .iter()
        .map(|entry| entry["connection_id"].as_str().unwrap())
        .collect();
    assert_eq!(connection_ids.len(), 2);
    assert!(
        logs.iter()
            .any(|entry| entry["thread_id"] == first.thread_id
                && entry["message"].as_str().unwrap().contains("session/load"))
    );
    assert!(
        logs.iter()
            .all(|entry| entry["thread_id"] != second.thread_id)
    );
    service.close_session(&first.thread_id).await?;
    assert!(!service.session_logs(&first.thread_id)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn url_elicitation_completion_is_logged_only_for_its_thread() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    let service = AcpService::new(correlation_host(), Some(workspace.join("state")))?;
    let connection = service.connect("test", workspace).await?;
    let first = service.new_session(&connection.id).await?;
    let second = service.new_session(&connection.id).await?;
    service.start_prompt(&first.thread_id, prompt("url-elicitation"))?;
    let pending = wait_for(&service, &first.thread_id, |snapshot| {
        !snapshot.permissions.is_empty()
    })
    .await;
    service.answer(
        &first.thread_id,
        &pending.permissions[0].id,
        json!({"action":"accept"}),
    )?;
    let completion = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(entry) = service
                .session_logs(&first.thread_id)
                .unwrap()
                .into_iter()
                .find(|entry| {
                    entry["message"]
                        .as_str()
                        .unwrap()
                        .contains("elicitation/complete")
                })
            {
                break entry;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(completion["thread_id"], first.thread_id);
    assert_eq!(
        completion["session_id"],
        first.session_id.as_deref().unwrap()
    );
    assert!(
        !service
            .session_logs(&second.thread_id)?
            .iter()
            .any(|entry| entry["message"]
                .as_str()
                .unwrap()
                .contains("elicitation/complete"))
    );
    service.delete_session(&first.thread_id).await?;
    assert!(
        service
            .logs(&connection.id)?
            .iter()
            .all(|entry| entry["thread_id"] != first.thread_id)
    );
    assert!(!service.session_logs(&second.thread_id)?.is_empty());
    service.start_prompt(&second.thread_id, prompt("after-deletion"))?;
    let pending = wait_for(&service, &second.thread_id, |snapshot| {
        !snapshot.permissions.is_empty()
    })
    .await;
    service.answer(
        &second.thread_id,
        &pending.permissions[0].id,
        json!({"outcome":{"outcome":"selected","optionId":"allow"}}),
    )?;
    wait_for(&service, &second.thread_id, |snapshot| {
        snapshot.status == "idle"
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn failed_connection_logs_are_bounded_and_removed_when_the_thread_is_deleted() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().canonicalize()?;
    tokio::fs::write(workspace.join("fail-initialize"), "").await?;
    let service = AcpService::new(correlation_host(), Some(workspace.join("state")))?;
    let thread = service.prepare_session("test", workspace).await?;
    assert!(service.resume(&thread.thread_id).await.is_err());
    let first_connection = service.session_logs(&thread.thread_id)?[0]["connection_id"]
        .as_str()
        .unwrap()
        .to_owned();
    for _ in 0..65 {
        assert!(service.resume(&thread.thread_id).await.is_err());
    }
    let logs = service.session_logs(&thread.thread_id)?;
    let retained: std::collections::HashSet<_> = logs
        .iter()
        .map(|entry| entry["connection_id"].as_str().unwrap())
        .collect();
    assert_eq!(retained.len(), 64);
    assert!(service.logs(&first_connection).is_err());
    assert_eq!(service.sessions(None).len(), 1);
    service.delete_session(&thread.thread_id).await?;
    for connection in retained {
        assert!(service.logs(connection).is_err());
    }
    assert!(service.session_logs(&thread.thread_id).is_err());
    Ok(())
}

#[tokio::test]
async fn sync_events_cover_adapter_callbacks_requests_binding_and_disconnect() -> Result<()> {
    use aow_acp::SyncResource;
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(false), None)?;
    let mut events = service.subscribe_sync();
    let connection = service.connect("test", directory.path().into()).await?;
    let session = service.new_session(&connection.id).await?;
    let binding = SyncResource::Connection {
        thread_id: session.thread_id.clone(),
    };
    let requests = SyncResource::Requests {
        connection_id: connection.id.clone(),
    };
    let mut bound = false;
    while let Ok(event) = events.try_recv() {
        if event.resource == binding {
            bound = true;
        }
    }
    assert!(bound);
    assert_eq!(
        service
            .session_connection_info(&session.thread_id)?
            .unwrap()
            .id,
        connection.id
    );
    let authenticating = service.clone();
    let id = connection.id.clone();
    let auth = tokio::spawn(async move { authenticating.authenticate(&id, "login").await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while events.recv().await.unwrap().resource != requests {}
    })
    .await?;
    let pending = service.connection_requests(&connection.id)?;
    assert_eq!(pending.len(), 1);
    service.answer_connection(
        &connection.id,
        &pending[0].id,
        json!({"action":"accept", "content":{"code":"1234"}}),
    )?;
    assert!(service.connection_requests(&connection.id)?.is_empty());
    auth.await??;
    let mut answered = 0;
    while let Ok(event) = events.try_recv() {
        if event.resource == requests {
            answered += 1;
        }
    }
    assert_eq!(
        answered, 1,
        "answer and dispatcher cleanup must not double-notify the same removal"
    );
    service.start_prompt(&session.thread_id, prompt("message-ids"))?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if let Some(head) = event.session
                && head.info.thread_id == session.thread_id
                && head.info.status == "idle"
            {
                let snapshot = service.snapshot(&session.thread_id).unwrap();
                assert!(snapshot.revision >= head.revision);
                assert!(
                    snapshot
                        .entries
                        .iter()
                        .any(|entry| entry.content["text"] == "ACP_LIVE_FOLLOWUP_VISIBLE")
                );
                break;
            }
        }
    })
    .await?;
    service.disconnect(&connection.id)?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.resource == binding {
                break;
            }
        }
    })
    .await?;
    assert!(
        service
            .session_connection_info(&session.thread_id)?
            .is_none()
    );
    assert!(service.sync_event(&requests).deleted);
    Ok(())
}
