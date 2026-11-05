use anyhow::Result;
use aow_acp_runtime_core::{AcpHost, AcpService, Content, SessionCreation, SessionSource};
use aow_host_contract::instance::InstanceRef;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Host;

#[tokio::test]
async fn managed_prompt_retries_do_not_repeat_work_after_completion_replay_or_restart() -> Result<()>
{
    let directory = tempfile::tempdir()?;
    let history = directory.path().join("history");
    let service = AcpService::new(Arc::new(Host), Some(history.clone()))?;
    let session = service
        .prepare_session("test", directory.path().into())
        .await?;
    let id = &session.thread_id;
    service.resume(id).await?;
    let content = |text: &str| {
        serde_json::from_value::<Vec<Content>>(json!([{"type":"text","text":text}])).unwrap()
    };
    service.start_prompt_once(id, "one", content("wait"))?;
    service.start_prompt_once(id, "one", content("wait"))?;
    assert!(
        service
            .start_prompt_once(id, "two", content("second"))
            .is_err()
    );
    service.cancel(id)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while service.snapshot(id).unwrap().prompt_receipts["one"].status == "in_progress" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    assert_eq!(
        service.snapshot(id)?.prompt_receipts["one"].status,
        "interrupted"
    );
    service.start_prompt_once(id, "two", content("second"))?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while service.snapshot(id).unwrap().prompt_receipts["two"].status == "in_progress" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    service.close_session(id).await?;
    service.flush_threads_on_quit()?;
    drop(service);
    let service = AcpService::new(Arc::new(Host), Some(history))?;
    service.start_prompt_once(id, "two", content("second"))?;
    service.resume(id).await?;
    service.start_prompt_once(id, "one", content("wait"))?;
    assert_eq!(
        service.snapshot(id)?.prompt_receipts["two"]
            .conclusion
            .as_deref(),
        Some("Done")
    );
    let log = std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl"))?;
    assert_eq!(log.lines().count(), 2);
    service.close_session(id).await?;
    Ok(())
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/acp/lineage-agent.mjs")],
            "env":{"AOW_STATE_DIR":"wrong","AOW_INSTANCE":"terminal:stale","AOW_SESSION_ID":"stale","AOW_ACP_CONNECTION_ID":"stale"}}}}).to_string())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    fn execution_environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("AOW_STATE_DIR".into(), "/correct".into())])
    }

    async fn read_text_file(&self, _: &Path, _: &Path) -> Result<String> {
        unreachable!()
    }
    async fn write_text_file(&self, _: &Path, _: &Path, _: &str) -> Result<()> {
        unreachable!()
    }
}

#[tokio::test]
async fn concurrent_imports_register_each_native_session_once() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let service = AcpService::new(Arc::new(Host), None)?;
    let connection = service.connect("test", directory.path().into()).await?;
    let sessions: Vec<_> = (0..1000)
        .map(|index| aow_acp_runtime_core::SessionImport {
            session_id: format!("import-{index}"),
            cwd: connection.cwd.clone(),
            title: None,
            updated_at: None,
        })
        .collect();
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let mut imports = Vec::new();
    for _ in 0..4 {
        let service = service.clone();
        let connection_id = connection.id.clone();
        let sessions = sessions.clone();
        let barrier = barrier.clone();
        imports.push(tokio::task::spawn_blocking(move || {
            barrier.wait();
            service.import_sessions(&connection_id, sessions)
        }));
    }
    let mut imported = 0;
    for task in imports {
        imported += task.await??.len();
    }
    assert_eq!(imported, sessions.len());
    assert_eq!(service.sessions(None).len(), sessions.len());
    assert!(
        service
            .import_sessions(&connection.id, sessions)?
            .is_empty()
    );
    service.disconnect(&connection.id)?;
    Ok(())
}

#[tokio::test]
async fn native_session_ids_are_scoped_to_live_connections() -> Result<()> {
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir()?;
    let service = AcpService::new(Arc::new(Host), None)?;
    let a = service.connect("test", first.path().into()).await?;
    let b = service.connect("test", second.path().into()).await?;
    let left = service.load_session(&a.id, "same-native-id").await?;
    let right = service.load_session(&b.id, "same-native-id").await?;
    assert_ne!(left.thread_id, right.thread_id);
    assert_eq!(
        service.thread_for_session(&a.id, "same-native-id")?,
        left.thread_id
    );
    assert_eq!(
        service.thread_for_session(&b.id, "same-native-id")?,
        right.thread_id
    );
    service.close_session(&left.thread_id).await?;
    assert!(service.thread_for_session(&a.id, "same-native-id").is_err());
    assert_eq!(
        service.thread_for_session(&b.id, "same-native-id")?,
        right.thread_id
    );
    service.disconnect(&b.id)?;
    assert!(service.thread_for_session(&b.id, "same-native-id").is_err());
    Ok(())
}

#[tokio::test]
async fn parent_survives_lazy_reads_reload_and_shared_adapter_prompts_keep_their_identity()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let storage = directory.path().join("acp");
    let service = AcpService::new(Arc::new(Host), Some(storage.clone()))?;
    let root = service
        .prepare_session_with_options(
            "test",
            directory.path().into(),
            SessionCreation {
                source: SessionSource::Cli,
                parent: Some(InstanceRef::terminal("root")),
                ..Default::default()
            },
        )
        .await?;
    let child = service
        .prepare_session_with_options(
            "test",
            directory.path().into(),
            SessionCreation {
                source: SessionSource::User,
                parent: Some(InstanceRef::acp(&root.thread_id)),
                ..Default::default()
            },
        )
        .await?;
    for id in [&root.thread_id, &child.thread_id] {
        service.resume(id).await?;
        let content: Vec<Content> =
            serde_json::from_value(json!([{"type":"text","text":"hello"}]))?;
        service.start_prompt(id, content)?;
        tokio::time::timeout(Duration::from_secs(5), async {
            while service.snapshot(id).unwrap().status != "idle" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(
            service
                .snapshot(id)?
                .entries
                .iter()
                .filter(|entry| entry.kind == "user")
                .count(),
            1
        );
    }
    assert_eq!(
        service
            .session_connection_info(&root.thread_id)?
            .unwrap()
            .id,
        service
            .session_connection_info(&child.thread_id)?
            .unwrap()
            .id
    );
    let prompts: Vec<Value> =
        std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl"))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
    for (prompt, id) in prompts.iter().zip([&root.thread_id, &child.thread_id]) {
        assert_eq!(prompt["prompt"], json!([{"type":"text", "text":"hello"}]));
        assert!(prompt["caller_session"].is_null());
        let connection = service.session_connection_info(id)?.unwrap();
        assert_eq!(prompt["connection"], connection.id);
        assert_eq!(
            service.thread_for_session(&connection.id, prompt["sessionId"].as_str().unwrap())?,
            *id
        );
        assert!(
            service
                .thread_for_session(&connection.id, "unknown")
                .is_err()
        );
        assert_eq!(prompt["state_dir"], "/correct");
        assert!(prompt["instance"].is_null());
    }
    service.flush_threads_on_quit()?;
    let reloaded = AcpService::new(Arc::new(Host), Some(storage))?;
    // Metadata-only list/get must work before the transcript is hydrated.
    assert_eq!(
        reloaded.session_info(&child.thread_id)?.parent,
        child.parent
    );
    assert_eq!(
        reloaded
            .sessions(None)
            .iter()
            .find(|s| s.thread_id == root.thread_id)
            .unwrap()
            .parent,
        root.parent
    );
    assert_eq!(
        reloaded.resume(&child.thread_id).await?.parent,
        child.parent
    );
    assert_eq!(reloaded.snapshot(&child.thread_id)?.parent, child.parent);
    for expected in [&root, &child] {
        let info = reloaded.session_info(&expected.thread_id)?;
        assert_eq!(info.source, expected.source);
        assert_eq!(info.created_at, expected.created_at);
        let resumed = reloaded.resume(&expected.thread_id).await?;
        assert_eq!(resumed.source, expected.source);
        assert_eq!(resumed.created_at, expected.created_at);
    }
    let replayed = reloaded.snapshot(&child.thread_id)?;
    assert_eq!(
        replayed
            .entries
            .iter()
            .filter(|entry| entry.kind == "user")
            .map(|entry| entry.content["text"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["hello"]
    );
    Ok(())
}

#[tokio::test]
async fn execution_environments_scope_connections_and_native_sessions_and_survive_recovery()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let storage = directory.path().join("acp");
    for home in ["first", "second"] {
        std::fs::create_dir(directory.path().join(home))?;
    }
    let make_environment = |home: &str| aow_acp_runtime_core::ExecutionEnvironment {
        home: directory.path().join(home),
        env: BTreeMap::from([("CUSTOM_AGENT_SETTING".into(), home.into())]),
    };
    let service = AcpService::new(Arc::new(Host), Some(storage.clone()))?;
    let mut sessions = Vec::new();
    for execution_environment in [
        Some(make_environment("first")),
        Some(make_environment("first")),
        Some(make_environment("second")),
        None,
    ] {
        let thread = service
            .prepare_session_with_options(
                "test",
                directory.path().into(),
                SessionCreation {
                    execution_environment,
                    ..Default::default()
                },
            )
            .await?;
        service.resume(&thread.thread_id).await?;
        sessions.push(thread.data.thread_id);
    }
    let connection = |id: &str| service.session_connection_info(id).unwrap().unwrap().id;
    assert_eq!(connection(&sessions[0]), connection(&sessions[1]));
    assert_ne!(connection(&sessions[0]), connection(&sessions[2]));
    assert_ne!(connection(&sessions[0]), connection(&sessions[3]));
    let child = service
        .prepare_session_with_options(
            "test",
            directory.path().into(),
            SessionCreation {
                parent: Some(InstanceRef::acp(&sessions[0])),
                ..Default::default()
            },
        )
        .await?;
    assert!(
        child.execution_environment.is_none(),
        "parentage never chooses a child's execution environment"
    );
    let mut changed = make_environment("first");
    changed
        .env
        .insert("CUSTOM_AGENT_SETTING".into(), "changed".into());
    let changed = service
        .prepare_session_with_options(
            "test",
            directory.path().into(),
            SessionCreation {
                execution_environment: Some(changed),
                parent: Some(InstanceRef::acp(&sessions[0])),
                ..Default::default()
            },
        )
        .await?;
    service.resume(&changed.thread_id).await?;
    assert_ne!(connection(&changed.thread_id), connection(&sessions[0]));
    let content = || serde_json::from_value(json!([{"type":"text","text":"hello"}])).unwrap();
    for id in &sessions {
        service.start_prompt(id, content())?;
        tokio::time::timeout(Duration::from_secs(5), async {
            while service.snapshot(id).unwrap().status != "idle" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
    }
    let prompts: Vec<Value> =
        std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl"))?
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(
        prompts[0]["home"],
        make_environment("first").home.to_string_lossy().as_ref()
    );
    assert_eq!(prompts[0]["setting"], "first");
    assert_eq!(prompts[2]["setting"], "second");
    for prompt in &prompts {
        assert_eq!(prompt["prompt"], json!([{"type":"text","text":"hello"}]));
    }
    let left = service
        .load_session(&connection(&sessions[0]), "same-native-id")
        .await?;
    let right = service
        .load_session(&connection(&sessions[2]), "same-native-id")
        .await?;
    assert_ne!(left.thread_id, right.thread_id);
    assert_eq!(
        service
            .load_session(&connection(&sessions[0]), "same-native-id")
            .await?
            .thread_id,
        left.thread_id
    );
    let forked = service.fork_session(&sessions[0]).await?;
    assert_eq!(
        forked.execution_environment,
        Some(make_environment("first"))
    );
    for id in [&sessions[0], &sessions[2]] {
        let imported = service.import_sessions(
            &connection(id),
            vec![aow_acp_runtime_core::SessionImport {
                session_id: "imported-native-id".into(),
                cwd: directory
                    .path()
                    .canonicalize()?
                    .to_string_lossy()
                    .into_owned(),
                title: None,
                updated_at: None,
            }],
        )?;
        assert_eq!(imported.len(), 1);
        assert_eq!(
            service
                .snapshot(&imported[0].thread_id)?
                .execution_environment,
            service.snapshot(id)?.execution_environment
        );
    }
    service.flush_threads_on_quit()?;
    let connections: std::collections::HashSet<_> = sessions
        .iter()
        .chain([&changed.thread_id])
        .map(|id| connection(id))
        .collect();
    for id in connections {
        service.disconnect(&id)?;
    }
    drop(service);
    let restored = AcpService::new(Arc::new(Host), Some(storage))?;
    assert_eq!(
        restored.snapshot(&sessions[0])?.execution_environment,
        Some(make_environment("first"))
    );
    assert_eq!(
        restored.resume(&left.thread_id).await?.thread_id,
        left.thread_id
    );
    assert_eq!(
        restored.resume(&right.thread_id).await?.thread_id,
        right.thread_id
    );
    restored.resume(&sessions[0]).await?;
    restored.start_prompt(&sessions[0], content())?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while restored.snapshot(&sessions[0]).unwrap().status != "idle" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let lines = std::fs::read_to_string(directory.path().join("lineage-prompts.jsonl"))?;
    let last: Value = serde_json::from_str(lines.lines().last().unwrap())?;
    assert_eq!(last["home"], prompts[0]["home"]);
    assert_eq!(last["prompt"], prompts[0]["prompt"]);
    // Recovery must fail instead of falling back to the host Home when the supplied Home disappears.
    restored.disconnect(&restored.session_connection_info(&sessions[0])?.unwrap().id)?;
    std::fs::remove_dir(directory.path().join("first"))?;
    assert!(restored.resume(&sessions[0]).await.is_err());
    assert!(!directory.path().join("first").exists());
    Ok(())
}
