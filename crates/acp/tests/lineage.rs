use anyhow::Result;
use aow_acp::{AcpHost, AcpService, Content, SessionCreation, SessionSource};
use aow_protocol::InstanceRef;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Host;
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lineage-agent.mjs")],
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
