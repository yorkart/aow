use anyhow::Result;
use aow_acp::{AcpHost, AcpService, Content, SessionSnapshot};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Host(Value);
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(self.0.to_string())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &Path, _: &Path) -> Result<String> {
        unreachable!()
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> Result<()> {
        unreachable!()
    }
}

fn host(env: Value) -> Arc<Host> {
    Arc::new(Host(
        json!({"agent_servers":{"test":{"type":"custom","command":"node",
        "args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fork-agent.mjs")], "env":env,
        "default_mode":"code", "default_config_options":{"model":"default"}}}}),
    ))
}

async fn prompt(service: &AcpService, id: &str, text: &str) -> Result<SessionSnapshot> {
    service.start_prompt(
        id,
        serde_json::from_value::<Vec<Content>>(json!([{"type":"text","text":text}]))?,
    )?;
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let snapshot = service.snapshot(id)?;
            if snapshot.status != "working" {
                return Ok(snapshot);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?
}

async fn fork_started(directory: &Path) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !std::fs::read_to_string(directory.join("fork-requests.jsonl"))
            .unwrap_or_default()
            .contains("session/fork")
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn pending_fork_reserves_source_prompt_admission_without_blocking_other_sessions()
-> Result<()> {
    for no_load in [false, true] {
        let directory = tempfile::tempdir()?;
        let service = AcpService::new(host(json!({"NO_LOAD": if no_load {"1"} else {"0"}})), None)?;
        let connection = service.connect("test", directory.path().into()).await?;
        let source = service.new_session(&connection.id).await?;
        let source = prompt(&service, &source.thread_id, "shared-context").await?;
        let before = serde_json::to_value(&source)?;
        std::fs::write(directory.path().join("hold-fork"), "")?;
        let caller_service = service.clone();
        let id = source.thread_id.clone();
        let caller = tokio::spawn(async move { caller_service.fork_session(&id).await });
        fork_started(directory.path()).await?;
        let error = service
            .start_prompt(
                &source.thread_id,
                serde_json::from_value(json!([{"type":"text","text":"racing-prompt"}]))?,
            )
            .unwrap_err();
        assert!(error.to_string().contains("being forked"));
        assert!(service.fork_session(&source.thread_id).await.is_err());
        assert_eq!(
            serde_json::to_value(service.snapshot(&source.thread_id)?)?,
            before
        );
        let other = service.new_session(&connection.id).await?;
        assert_eq!(
            prompt(&service, &other.thread_id, "unrelated")
                .await?
                .stop_reason
                .as_deref(),
            Some("end_turn")
        );
        std::fs::remove_file(directory.path().join("hold-fork"))?;
        let fork = caller.await??;
        assert_eq!(
            fork.entries.iter().map(|e| &e.content).collect::<Vec<_>>(),
            source
                .entries
                .iter()
                .map(|e| &e.content)
                .collect::<Vec<_>>()
        );
        prompt(&service, &source.thread_id, "source-only").await?;
        let child = prompt(&service, &fork.thread_id, "child-only").await?;
        assert_eq!(
            child.entries.last().unwrap().content["text"],
            "History: shared-context; reply: child-only"
        );
    }
    Ok(())
}

#[tokio::test]
async fn fork_replay_preserves_verified_turn_outcomes_through_retry_and_restart() -> Result<()> {
    for (env, status) in [
        (json!({}), "completed"),
        (json!({"PROMPT_STOP_REASON":"max_tokens"}), "interrupted"),
        (json!({"PROMPT_ERROR":"1"}), "failed"),
    ] {
        for fail_load in [false, true] {
            let directory = tempfile::tempdir()?;
            let storage = directory.path().join("state");
            let host = host(env.clone());
            let service = AcpService::new(host.clone(), Some(storage.clone()))?;
            let connection = service.connect("test", directory.path().into()).await?;
            let source = service.new_session(&connection.id).await?;
            let source = prompt(&service, &source.thread_id, "question").await?;
            assert_eq!(source.turns[0].status, status);
            if fail_load {
                std::fs::write(directory.path().join("fail-fork-load"), "")?;
            }
            let fork = service.fork_session(&source.thread_id).await?;
            assert_eq!(fork.needs_load, fail_load);
            assert_eq!(fork.turns.len(), 1);
            assert_eq!(fork.turns[0].status, status);
            service.flush_threads_on_quit()?;
            service.disconnect(&connection.id)?;
            drop(service);
            if fail_load {
                std::fs::remove_file(directory.path().join("fail-fork-load"))?;
            }
            let service = AcpService::new(host, Some(storage))?;
            let restored = service.resume(&fork.thread_id).await?;
            for snapshot in [&fork, &restored] {
                assert_eq!(snapshot.turns.len(), 1);
                let turn = &snapshot.turns[0];
                assert_eq!(turn.user_entry_id, snapshot.entries[0].id);
                assert_eq!(turn.status, status);
                assert_eq!(turn.stop_reason, source.turns[0].stop_reason);
                assert_eq!(turn.error, source.turns[0].error);
            }
            let overview = service.overview(&fork.thread_id)?.overview.unwrap();
            assert_eq!(overview[0].status.as_deref(), Some(status));
            assert_eq!(overview[0].conclusion.is_empty(), status != "completed");
        }
    }
    Ok(())
}

#[tokio::test]
async fn fork_replay_does_not_copy_outcomes_onto_different_native_history() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(
        host(json!({"PROMPT_STOP_REASON":"max_tokens", "FORK_REWRITE_HISTORY":"1"})),
        None,
    )?;
    let connection = service.connect("test", directory.path().into()).await?;
    let source = service.new_session(&connection.id).await?;
    let source = prompt(&service, &source.thread_id, "question").await?;
    assert_eq!(source.turns[0].status, "interrupted");
    let fork = service.fork_session(&source.thread_id).await?;
    assert_eq!(
        fork.entries.last().unwrap().content["text"],
        "Different native result"
    );
    assert!(fork.turns.is_empty());
    Ok(())
}

#[tokio::test]
async fn fork_inherits_remote_history_and_keeps_identity_configuration_and_parent_after_restart()
-> Result<()> {
    for no_load in [false, true] {
        let directory = tempfile::tempdir()?;
        let storage = directory.path().join("state");
        let host = host(json!({"NO_LOAD":if no_load {"1"} else {"0"}}));
        let service = AcpService::new(host.clone(), Some(storage.clone()))?;
        let connection = service.connect("test", directory.path().into()).await?;
        let source = service.new_session(&connection.id).await?;
        let source = prompt(&service, &source.thread_id, "shared-context").await?;
        let before = serde_json::to_value(&source)?;
        let fork = service.fork_session(&source.thread_id).await?;
        assert_ne!(fork.thread_id, source.thread_id);
        assert_ne!(fork.session_id, source.session_id);
        assert_eq!(
            fork.parent,
            Some(aow_protocol::InstanceRef::acp(&source.thread_id))
        );
        assert_eq!(
            fork.modes["currentModeId"], "ask",
            "fork must retain returned mode instead of applying new-session defaults"
        );
        assert_eq!(fork.config_options[0]["currentValue"], "inherited");
        assert_eq!(
            fork.entries.iter().map(|e| &e.content).collect::<Vec<_>>(),
            source
                .entries
                .iter()
                .map(|e| &e.content)
                .collect::<Vec<_>>()
        );
        let requests = std::fs::read_to_string(directory.path().join("fork-requests.jsonl"))?;
        let request: Value = requests
            .lines()
            .map(serde_json::from_str::<Value>)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|r| r["method"] == "session/fork")
            .unwrap();
        assert_eq!(
            request["params"],
            json!({"sessionId":source.session_id,"cwd":source.cwd,"mcpServers":[]})
        );
        let child = prompt(&service, &fork.thread_id, "child-only").await?;
        assert!(
            child
                .entries
                .iter()
                .any(|e| e.content["text"] == "History: shared-context; reply: child-only")
        );
        assert_eq!(
            serde_json::to_value(service.snapshot(&source.thread_id)?)?,
            before
        );
        service.flush_threads_on_quit()?;
        service.disconnect(&connection.id)?;
        drop(service);
        let restored = AcpService::new(host, Some(storage))?;
        assert_eq!(restored.session_info(&fork.thread_id)?.parent, fork.parent);
        let resumed = restored.resume(&fork.thread_id).await?;
        assert_eq!(resumed.session_id, fork.session_id);
        assert!(
            resumed
                .entries
                .iter()
                .any(|e| e.content["text"] == "child-only")
        );
    }
    Ok(())
}

#[tokio::test]
async fn unsupported_or_failed_forks_do_not_create_blank_sessions_or_change_the_source()
-> Result<()> {
    for env in [
        json!({"FORK_CAPABILITY":"null"}),
        json!({"FORK_ERROR":"1"}),
        json!({"FORK_SAME_ID":"1"}),
    ] {
        let directory = tempfile::tempdir()?;
        let service = AcpService::new(host(env), None)?;
        let connection = service.connect("test", directory.path().into()).await?;
        let source = service.new_session(&connection.id).await?;
        let before = serde_json::to_value(&source)?;
        assert!(service.fork_session(&source.thread_id).await.is_err());
        assert_eq!(service.sessions(None).len(), 1);
        assert_eq!(
            serde_json::to_value(service.snapshot(&source.thread_id)?)?,
            before
        );
        assert_eq!(
            prompt(&service, &source.thread_id, "after-failed-fork")
                .await?
                .stop_reason
                .as_deref(),
            Some("end_turn")
        );
    }
    Ok(())
}

#[tokio::test]
async fn active_and_disconnected_sessions_cannot_be_forked() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(json!({})), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let source = service.new_session(&connection.id).await?;
    service.start_prompt(
        &source.thread_id,
        serde_json::from_value(json!([{"type":"text","text":"hold"}]))?,
    )?;
    assert!(service.fork_session(&source.thread_id).await.is_err());
    service.close_session(&source.thread_id).await?;
    assert!(service.fork_session(&source.thread_id).await.is_err());
    assert_eq!(service.sessions(None).len(), 1);
    assert!(
        !std::fs::read_to_string(directory.path().join("fork-requests.jsonl"))?
            .contains("session/fork")
    );
    Ok(())
}

#[tokio::test]
async fn fork_settles_after_caller_leaves_and_replay_failure_keeps_a_recoverable_native_identity()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(host(json!({})), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let source = service.new_session(&connection.id).await?;
    let original = prompt(&service, &source.thread_id, "context").await?;
    std::fs::write(directory.path().join("fail-fork-load"), "")?;
    std::fs::write(directory.path().join("hold-fork"), "")?;
    let caller_service = service.clone();
    let caller = tokio::spawn(async move { caller_service.fork_session(&source.thread_id).await });
    fork_started(directory.path()).await?;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(
        service
            .start_prompt(
                &original.thread_id,
                serde_json::from_value(json!([{"type":"text","text":"while-abandoned"}]))?
            )
            .is_err()
    );
    std::fs::remove_file(directory.path().join("hold-fork"))?;
    let fork = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(child) = service
                .sessions(None)
                .into_iter()
                .find(|s| s.thread_id != original.thread_id)
            {
                let snapshot = service.snapshot(&child.thread_id).unwrap();
                if snapshot
                    .recovery
                    .as_ref()
                    .is_some_and(|r| r.phase == "failed")
                {
                    break snapshot;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(fork.session_id.is_some());
    assert!(fork.needs_load);
    assert_eq!(fork.status, "disconnected");
    assert!(service.start_prompt(&fork.thread_id, vec![]).is_err());
    // The recovery task publishes failure before the detached fork task returns.
    // Wait for source admission to be released as well.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match prompt(&service, &original.thread_id, "after-fork-settled").await {
                Err(error) if error.to_string().contains("being forked") => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                result => {
                    assert_eq!(result?.stop_reason.as_deref(), Some("end_turn"));
                    return Ok::<(), anyhow::Error>(());
                }
            }
        }
    })
    .await??;
    std::fs::remove_file(directory.path().join("fail-fork-load"))?;
    let restored = service.resume(&fork.thread_id).await?;
    assert_eq!(restored.session_id, fork.session_id);
    assert!(
        restored
            .entries
            .iter()
            .any(|e| e.content["text"] == "context")
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires an authenticated codex-acp command in AOW_ACP_FORK_COMMAND; runs two inference turns"]
async fn live_codex_acp_fork_preserves_context_and_resumes() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let storage = directory.path().join("state");
    let command = std::env::var("AOW_ACP_FORK_COMMAND")?;
    let host = Arc::new(Host(
        json!({"agent_servers":{"codex-acp":{"type":"custom","command":command}}}),
    ));
    let service = AcpService::new(host.clone(), Some(storage.clone()))?;
    let connection = service
        .connect("codex-acp", directory.path().into())
        .await?;
    assert_eq!(
        connection.capabilities["sessionCapabilities"]["fork"],
        json!({})
    );
    println!("codex-acp: {}", connection.agent_info);
    let source = service.new_session(&connection.id).await?;
    let marker = format!("AOW_FORK_{}", uuid::Uuid::new_v4().simple());
    let source = prompt(&service, &source.thread_id, &format!("Remember this exact token for a later question: {marker}. Reply only OK. Do not use tools.")).await?;
    assert_eq!(
        source.stop_reason.as_deref(),
        Some("end_turn"),
        "{:?}",
        source.error
    );
    let fork = service.fork_session(&source.thread_id).await?;
    assert_ne!(fork.session_id, source.session_id);
    assert!(fork.entries.iter().any(|e| {
        e.content["text"]
            .as_str()
            .is_some_and(|s| s.contains(&marker))
    }));
    let child = prompt(
        &service,
        &fork.thread_id,
        "What exact token did I ask you to remember? Reply with that token only. Do not use tools.",
    )
    .await?;
    assert_eq!(
        child.stop_reason.as_deref(),
        Some("end_turn"),
        "{:?}",
        child.error
    );
    assert!(
        child
            .entries
            .iter()
            .rev()
            .find(|e| e.kind == "assistant")
            .unwrap()
            .content["text"]
            .as_str()
            .unwrap()
            .contains(&marker)
    );
    // Codex may publish a generated title after the source turn completes.
    // Compare conversation history and identity rather than asynchronous metadata.
    let unchanged = service.snapshot(&source.thread_id)?;
    assert_eq!(unchanged.session_id, source.session_id);
    assert_eq!(
        serde_json::to_value(unchanged.entries)?,
        serde_json::to_value(source.entries)?
    );
    assert_eq!(
        serde_json::to_value(unchanged.turns)?,
        serde_json::to_value(source.turns)?
    );
    service.flush_threads_on_quit()?;
    service.disconnect(&connection.id)?;
    drop(service);
    let service = AcpService::new(host, Some(storage))?;
    let restored = service.resume(&fork.thread_id).await?;
    assert_eq!(restored.session_id, fork.session_id);
    assert_eq!(restored.parent, fork.parent);
    assert!(restored.entries.iter().any(|e| {
        e.content["text"]
            .as_str()
            .is_some_and(|s| s.contains(&marker))
    }));
    println!("Verified fork context, source isolation, and restart recovery");
    Ok(())
}
