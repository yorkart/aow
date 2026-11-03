use anyhow::Result;
use aow_acp::{AcpHost, AcpService, Content, SessionImport, SessionSnapshot};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Host(String);
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn settings(&self) -> Result<String> {
        Ok(self.0.clone())
    }
    async fn read_text_file(&self, _: &Path, path: &Path) -> Result<String> {
        Ok(tokio::fs::read_to_string(path).await?)
    }
    async fn write_text_file(&self, _: &Path, path: &Path, text: &str) -> Result<()> {
        Ok(tokio::fs::write(path, text).await?)
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    service: AcpService,
    connection: String,
    thread: String,
}

impl Fixture {
    async fn new(env: Value) -> Result<Self> {
        Self::with_storage(env, false).await
    }
    async fn with_storage(env: Value, persistent: bool) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let service = AcpService::new(Arc::new(Host(json!({"agent_servers":{"test":{
            "type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/load-recovery-agent.mjs")], "env":env,
        }}}).to_string())), persistent.then(|| dir.path().join("state")))?;
        let connection = service.connect("test", dir.path().into()).await?.id;
        let mut fixture = Self {
            dir,
            service,
            connection,
            thread: String::new(),
        };
        fixture.thread = fixture.import("target")?;
        Ok(fixture)
    }
    fn import(&self, session_id: &str) -> Result<String> {
        Ok(self.service.import_sessions(
            &self.connection,
            vec![SessionImport {
                session_id: session_id.into(),
                cwd: self.dir.path().to_string_lossy().into_owned(),
                title: None,
                updated_at: None,
            }],
        )?[0]
            .thread_id
            .clone())
    }
    fn file(&self, name: &str, contents: &str) {
        std::fs::write(self.dir.path().join(name), contents).unwrap();
    }
    fn remove(&self, name: &str) {
        std::fs::remove_file(self.dir.path().join(name)).unwrap();
    }
    fn events(&self, method: &str, sid: &str, stage: &str) -> Vec<Value> {
        std::fs::read_to_string(self.dir.path().join("recovery-events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| {
                event["method"] == method && event["sessionId"] == sid && event["stage"] == stage
            })
            .collect()
    }
    async fn event(&self, method: &str, sid: &str, stage: &str, count: usize) {
        until(|| self.events(method, sid, stage).len() >= count).await;
    }
    fn load(&self) -> tokio::task::JoinHandle<Result<SessionSnapshot>> {
        let service = self.service.clone();
        let thread = self.thread.clone();
        tokio::spawn(async move { service.resume(&thread).await })
    }
    fn snapshot(&self) -> SessionSnapshot {
        self.service.snapshot(&self.thread).unwrap()
    }
    async fn settled(&self) {
        until(|| {
            self.snapshot()
                .recovery
                .as_ref()
                .is_some_and(|state| state.phase == "failed")
        })
        .await;
    }
    async fn seed_history(&self) -> Result<SessionSnapshot> {
        let snapshot = self.service.resume(&self.thread).await?;
        self.service.close_session(&self.thread).await?;
        Ok(snapshot)
    }
    async fn second_connection(&self) -> Result<String> {
        let keeper = self.import("keeper")?;
        self.service.resume(&keeper).await?;
        let other = self.import("other")?;
        self.file("fail-load", "failed");
        assert!(self.service.resume(&other).await.is_err());
        self.remove("fail-load");
        self.service.resume(&other).await?;
        let second = self.service.session_connection_info(&other)?.unwrap().id;
        assert_ne!(second, self.connection);
        Ok(second)
    }
}

#[tokio::test]
async fn failed_install_rolls_back_history_and_persistence_before_retry() -> Result<()> {
    for resume_only in [false, true] {
        let fixture =
            Fixture::with_storage(json!({"RESUME_ONLY":if resume_only {"1"} else {"0"}}), true)
                .await?;
        fixture.service.resume(&fixture.thread).await?;
        fixture
            .service
            .start_prompt(&fixture.thread, prompt("previous"))?;
        until(|| fixture.snapshot().status == "idle").await;
        let before = fixture.snapshot();
        fixture.service.close_session(&fixture.thread).await?;
        fixture.file("hold-load", "");
        fixture.file("answer.txt", "rejected replay");
        let load = fixture.load();
        let method = if resume_only {
            "session/resume"
        } else {
            "session/load"
        };
        fixture.event(method, "target", "start", 2).await;
        let resource = aow_acp::SyncResource::Session {
            thread_id: fixture.thread.clone(),
        };
        let loading_version = fixture.service.sync_event(&resource).version;
        let database = rusqlite::Connection::open(fixture.dir.path().join("state/threads.sqlite"))?;
        database.execute_batch(include_str!("sql/reject_snapshot_writes.sql"))?;
        fixture.file("release-load", "");
        let error = load.await?.unwrap_err();
        assert!(format!("{error:#}").contains("test disk failure"));
        let failed = fixture.snapshot();
        assert_eq!(entries(&failed), entries(&before));
        assert_eq!(
            serde_json::to_value(&failed.turns)?,
            serde_json::to_value(&before.turns)?
        );
        assert_eq!(failed.status, "disconnected");
        assert!(failed.needs_load);
        assert!(failed.error.is_none());
        assert_eq!(failed.recovery.as_ref().unwrap().phase, "failed");
        assert!(
            failed
                .recovery
                .as_ref()
                .unwrap()
                .error
                .as_deref()
                .unwrap()
                .contains("test disk failure")
        );
        assert!(
            fixture.service.sync_event(&resource).version > loading_version,
            "disk errors must still invalidate the UI snapshot"
        );
        assert!(
            fixture
                .service
                .start_prompt(&fixture.thread, prompt("too early"))
                .is_err()
        );
        assert_eq!(
            fixture.events("session/close", "target", "complete").len(),
            2
        );

        database.execute_batch(include_str!("sql/allow_snapshot_writes.sql"))?;
        fixture.service.flush_threads_on_quit()?;
        let stored: Vec<u8> = database.query_row(
            include_str!("sql/snapshot_data.sql"),
            [&fixture.thread],
            |row| row.get(0),
        )?;
        let stored: Value = serde_json::from_slice(&stored)?;
        assert_eq!(
            stored["entries"],
            entries(&before),
            "writer retries must not commit the rejected replay"
        );
        assert_eq!(stored["status"], "disconnected");
        assert_eq!(stored["needs_load"], true);
        fixture.file("answer.txt", "accepted replay");
        let restored = fixture.service.resume(&fixture.thread).await?;
        assert!(restored.recovery.is_none());
        assert_eq!(fixture.events(method, "target", "start").len(), 3);
        fixture
            .service
            .start_prompt(&fixture.thread, prompt("next"))?;
        until(|| fixture.snapshot().status == "idle").await;
        assert!(fixture.snapshot().error.is_none());
        assert_eq!(
            fixture.snapshot().entries.last().unwrap().content["text"],
            "prompt complete"
        );
    }
    Ok(())
}

#[tokio::test]
async fn returning_to_an_old_connection_closes_stale_session_before_replay() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    let second = fixture.second_connection().await?;
    fixture
        .service
        .load_session(&fixture.connection, "target")
        .await?;
    fixture.service.load_session(&second, "target").await?;
    fixture.service.close_session(&fixture.thread).await?;
    fixture.file("hold-close", "");
    let first = fixture.service.load_session(&fixture.connection, "target");
    tokio::pin!(first);
    assert!(futures::poll!(&mut first).is_pending());
    fixture.event("session/close", "target", "start", 2).await;
    assert_eq!(fixture.snapshot().recovery.unwrap().phase, "cleaning");
    let retry = fixture.service.resume(&fixture.thread);
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    assert_eq!(fixture.events("session/load", "target", "start").len(), 2);
    fixture.file("answer.txt", "fresh replay");
    fixture.file("release-close", "");
    let restored = first.await?;
    assert_eq!(entries(&restored), entries(&retry.await?));
    assert_eq!(restored.status, "idle");
    assert!(!restored.needs_load);
    assert!(restored.recovery.is_none());
    assert_eq!(restored.entries.len(), 3);
    assert_eq!(
        restored.entries.last().unwrap().content["text"],
        "fresh replay"
    );
    assert_eq!(
        fixture
            .service
            .session_connection_info(&fixture.thread)?
            .unwrap()
            .id,
        fixture.connection
    );
    assert_eq!(fixture.events("session/load", "target", "start").len(), 3);
    assert_eq!(
        fixture.events("session/close", "target", "complete").len(),
        2
    );
    fixture
        .service
        .start_prompt(&fixture.thread, prompt("next"))?;
    until(|| fixture.snapshot().status == "idle").await;
    assert!(fixture.snapshot().error.is_none());
    assert_eq!(
        fixture.snapshot().entries.last().unwrap().content["text"],
        "prompt complete"
    );
    Ok(())
}

#[tokio::test]
async fn stale_loaded_session_cleanup_failure_can_retry_on_another_adapter() -> Result<()> {
    for no_close in [false, true] {
        let fixture = Fixture::new(json!({"NO_CLOSE":if no_close {"1"} else {"0"}})).await?;
        let second = fixture.second_connection().await?;
        fixture
            .service
            .load_session(&fixture.connection, "target")
            .await?;
        // Move the binding without leaving a second loaded remote session behind.
        fixture.file("fail-load", "auth");
        assert!(
            fixture
                .service
                .load_session(&second, "target")
                .await
                .is_err()
        );
        fixture.remove("fail-load");
        let before = fixture.snapshot();
        fixture.file("fail-close", "");
        assert!(
            fixture
                .service
                .load_session(&fixture.connection, "target")
                .await
                .is_err()
        );
        assert_eq!(entries(&fixture.snapshot()), entries(&before));
        assert_eq!(fixture.snapshot().recovery.unwrap().phase, "failed");
        assert_eq!(
            fixture.events("session/load", "target", "start").len(),
            2,
            "a failed close must not be followed by another load on that connection"
        );
        fixture.remove("fail-close");
        fixture.service.resume(&fixture.thread).await?;
        assert_ne!(
            fixture
                .service
                .session_connection_info(&fixture.thread)?
                .unwrap()
                .id,
            fixture.connection
        );
        fixture
            .service
            .start_prompt(&fixture.thread, prompt("next"))?;
        until(|| fixture.snapshot().status == "idle").await;
        assert!(fixture.snapshot().error.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn reimported_thread_cannot_reuse_an_old_threads_loaded_cache() -> Result<()> {
    let mut fixture = Fixture::new(json!({})).await?;
    let second = fixture.second_connection().await?;
    fixture
        .service
        .load_session(&fixture.connection, "target")
        .await?;
    fixture.service.load_session(&second, "target").await?;
    let previous_thread = fixture.thread.clone();
    fixture.service.delete_session(&previous_thread).await?;
    fixture.thread = fixture.import("target")?;
    assert_ne!(fixture.thread, previous_thread);
    let restored = fixture
        .service
        .load_session(&fixture.connection, "target")
        .await?;
    assert_eq!(restored.thread_id, fixture.thread);
    assert_eq!(restored.status, "idle");
    assert_eq!(restored.entries.len(), 3);
    assert_eq!(fixture.events("session/load", "target", "start").len(), 3);
    fixture
        .service
        .start_prompt(&fixture.thread, prompt("next"))?;
    until(|| fixture.snapshot().status == "idle").await;
    assert!(fixture.snapshot().error.is_none());
    Ok(())
}

#[tokio::test]
async fn cancelled_stale_cache_cleanup_does_not_start_another_load() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    let second = fixture.second_connection().await?;
    fixture
        .service
        .load_session(&fixture.connection, "target")
        .await?;
    fixture.service.load_session(&second, "target").await?;
    fixture.service.close_session(&fixture.thread).await?;
    let before = fixture.snapshot();
    fixture.file("hold-close", "");
    let pending = fixture.service.load_session(&fixture.connection, "target");
    tokio::pin!(pending);
    assert!(futures::poll!(&mut pending).is_pending());
    fixture.event("session/close", "target", "start", 2).await;
    fixture.service.cancel(&fixture.thread)?;
    assert!(pending.await.is_err());
    fixture.file("release-close", "");
    fixture.settled().await;
    assert_eq!(entries(&fixture.snapshot()), entries(&before));
    assert_eq!(fixture.events("session/load", "target", "start").len(), 2);
    assert_eq!(
        fixture.events("session/close", "target", "complete").len(),
        2
    );
    let restored = fixture.service.resume(&fixture.thread).await?;
    assert!(restored.recovery.is_none());
    assert_eq!(fixture.events("session/load", "target", "start").len(), 3);
    assert_eq!(
        fixture
            .service
            .session_connection_info(&fixture.thread)?
            .unwrap()
            .id,
        fixture.connection
    );
    Ok(())
}

#[tokio::test]
async fn stale_cache_setup_failure_keeps_the_remote_session_quarantined() -> Result<()> {
    let fixture = Fixture::with_storage(json!({}), true).await?;
    let second = fixture.second_connection().await?;
    fixture
        .service
        .load_session(&fixture.connection, "target")
        .await?;
    fixture.service.load_session(&second, "target").await?;
    fixture.service.close_session(&fixture.thread).await?;
    let before = fixture.snapshot();
    let database = rusqlite::Connection::open(fixture.dir.path().join("state/threads.sqlite"))?;
    database.execute_batch(include_str!("sql/reject_snapshot_writes.sql"))?;
    assert!(
        fixture
            .service
            .load_session(&fixture.connection, "target")
            .await
            .is_err()
    );
    assert_eq!(entries(&fixture.snapshot()), entries(&before));
    assert_eq!(fixture.snapshot().recovery.unwrap().phase, "failed");
    assert_eq!(fixture.events("session/load", "target", "start").len(), 2);
    assert_eq!(fixture.events("session/close", "target", "start").len(), 1);
    database.execute_batch(include_str!("sql/allow_snapshot_writes.sql"))?;
    let restored = fixture.service.resume(&fixture.thread).await?;
    assert!(restored.recovery.is_none());
    assert_eq!(
        fixture
            .service
            .session_connection_info(&fixture.thread)?
            .unwrap()
            .id,
        second
    );
    fixture
        .service
        .start_prompt(&fixture.thread, prompt("next"))?;
    until(|| fixture.snapshot().status == "idle").await;
    assert!(fixture.snapshot().error.is_none());
    Ok(())
}

async fn until(mut ready: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("mock event did not arrive");
}
fn prompt(text: &str) -> Vec<Content> {
    serde_json::from_value(json!([{"type":"text","text":text}])).unwrap()
}
fn entries(snapshot: &SessionSnapshot) -> Value {
    serde_json::to_value(&snapshot.entries).unwrap()
}

#[tokio::test]
async fn slow_load_survives_old_deadline_and_concurrent_retries_share_one_rpc() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    fixture.file("hold-load", "");
    let first = fixture.load();
    fixture.event("session/load", "target", "start", 1).await;
    let second = fixture.load();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::task::yield_now().await;
    assert!(
        !first.is_finished(),
        "load must outlive the old RPC deadline"
    );
    assert!(
        fixture.snapshot().entries.is_empty(),
        "partial replay is not published"
    );
    tokio::time::resume();
    fixture.file("release-load", "");
    let loaded = first.await??;
    assert_eq!(entries(&loaded), entries(&second.await??));
    assert_eq!(
        entries(&loaded),
        entries(&fixture.service.resume(&fixture.thread).await?)
    );
    assert_eq!(fixture.events("session/load", "target", "start").len(), 1);
    assert_eq!(loaded.entries.len(), 3);
    assert_eq!(
        loaded.entries.last().unwrap().message_id.as_deref(),
        Some("final-1")
    );
    fixture
        .service
        .start_prompt(&fixture.thread, prompt("next"))?;
    until(|| fixture.snapshot().status == "idle").await;
    assert!(fixture.snapshot().error.is_none());
    Ok(())
}

#[tokio::test]
async fn retries_keep_the_pending_connection_when_another_adapter_is_available() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    let keeper = fixture.import("keeper")?;
    fixture.service.resume(&keeper).await?;
    let other = fixture.import("other")?;
    fixture.file("fail-load", "failed");
    assert!(fixture.service.resume(&other).await.is_err());
    fixture.remove("fail-load");
    fixture.service.resume(&other).await?;
    let other_connection = fixture.service.session_connection_info(&other)?.unwrap().id;
    assert_ne!(other_connection, fixture.connection);

    fixture.file("hold-load", "");
    let first = fixture.service.load_session(&fixture.connection, "target");
    tokio::pin!(first);
    assert!(futures::poll!(&mut first).is_pending());
    fixture.event("session/load", "target", "start", 1).await;
    // An explicit attempt to change the connection cannot replace an in-flight route.
    assert!(
        fixture
            .service
            .load_session(&other_connection, "target")
            .await
            .is_err()
    );
    let retry = fixture.service.resume(&fixture.thread);
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    assert_eq!(
        fixture
            .service
            .session_connection_info(&fixture.thread)?
            .unwrap()
            .id,
        fixture.connection
    );
    fixture.file("release-load", "");
    assert_eq!(entries(&first.await?), entries(&retry.await?));
    assert_eq!(fixture.events("session/load", "target", "start").len(), 1);
    Ok(())
}

#[tokio::test]
async fn abandoned_load_waits_for_response_and_close_before_reopening() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    fixture.file("hold-load", "");
    fixture.file("hold-close", "");
    fixture.file("answer.txt", "old answer");
    let first = fixture.load();
    fixture.event("session/load", "target", "start", 1).await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    until(|| {
        fixture
            .snapshot()
            .recovery
            .is_some_and(|state| state.phase == "cleaning")
    })
    .await;
    let replacement = fixture.load();
    tokio::task::yield_now().await;
    assert_eq!(fixture.events("session/close", "target", "start").len(), 0);
    fixture.file("answer.txt", "new answer");
    fixture.file("release-load", "");
    fixture.event("session/close", "target", "start", 1).await;
    assert_eq!(fixture.events("session/load", "target", "start").len(), 1);
    assert!(!replacement.is_finished());
    fixture.file("release-close", "");
    let restored = replacement.await??;
    assert_eq!(restored.entries.len(), 3);
    assert_eq!(
        restored.entries.last().unwrap().content["text"],
        "new answer"
    );
    assert!(restored.recovery.is_none());
    assert_eq!(fixture.events("session/load", "target", "start").len(), 2);
    Ok(())
}

#[tokio::test]
async fn dropping_one_waiter_preserves_another_and_does_not_close_the_session() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    fixture.file("hold-load", "");
    let first = fixture.load();
    fixture.event("session/load", "target", "start", 1).await;
    let second = fixture.service.load_session(&fixture.connection, "target");
    tokio::pin!(second);
    assert!(futures::poll!(&mut second).is_pending());
    first.abort();
    let _ = first.await;
    fixture.file("release-load", "");
    second.await?;
    assert!(
        fixture
            .events("session/close", "target", "start")
            .is_empty()
    );
    assert_eq!(fixture.events("session/load", "target", "start").len(), 1);
    Ok(())
}

#[tokio::test]
async fn user_cancel_keeps_old_history_and_never_installs_late_success() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    let before = fixture.seed_history().await?;
    fixture.file("hold-load", "");
    fixture.file("answer.txt", "abandoned answer");
    let load = fixture.load();
    fixture.event("session/load", "target", "start", 2).await;
    fixture.service.cancel(&fixture.thread)?;
    assert!(load.await?.is_err());
    fixture.file("release-load", "");
    fixture.settled().await;
    assert_eq!(entries(&fixture.snapshot()), entries(&before));
    assert!(fixture.snapshot().error.is_none());
    let overview = fixture.service.overview(&fixture.thread)?.overview.unwrap();
    assert_eq!(
        overview.last().unwrap().conclusion.last().unwrap().content["text"],
        "complete answer"
    );
    fixture.file("answer.txt", "accepted answer");
    let restored = fixture.service.resume(&fixture.thread).await?;
    assert!(restored.recovery.is_none());
    assert_eq!(
        restored.entries.last().unwrap().content["text"],
        "accepted answer"
    );
    Ok(())
}

#[tokio::test]
async fn remote_load_errors_preserve_history_and_have_a_working_retry() -> Result<()> {
    for failure in ["cancelled", "load failed"] {
        let fixture = Fixture::new(json!({})).await?;
        let before = fixture.seed_history().await?;
        fixture.file("fail-load", failure);
        assert!(fixture.service.resume(&fixture.thread).await.is_err());
        assert_eq!(entries(&fixture.snapshot()), entries(&before));
        assert!(fixture.snapshot().error.is_none());
        assert_eq!(fixture.snapshot().recovery.unwrap().phase, "failed");
        fixture.remove("fail-load");
        assert!(
            fixture
                .service
                .resume(&fixture.thread)
                .await?
                .recovery
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test]
async fn failed_or_unsupported_close_moves_only_the_target_to_a_fresh_connection() -> Result<()> {
    for no_close in [false, true] {
        let fixture = Fixture::new(json!({"NO_CLOSE":if no_close {"1"} else {"0"}})).await?;
        let other = fixture.import("other")?;
        fixture.service.resume(&other).await?;
        fixture.file("hold-prompt", "");
        fixture.service.start_prompt(&other, prompt("wait"))?;
        fixture.event("session/prompt", "other", "start", 1).await;
        fixture.file("hold-load", "");
        fixture.file("fail-close", "");
        let load = fixture.load();
        fixture.event("session/load", "target", "start", 1).await;
        fixture.service.cancel(&fixture.thread)?;
        assert!(load.await?.is_err());
        fixture.file("release-load", "");
        fixture.settled().await;
        assert!(
            fixture
                .snapshot()
                .recovery
                .unwrap()
                .error
                .unwrap()
                .contains("cleanup failed")
        );
        let loaded = fixture.service.resume(&fixture.thread).await?;
        let target_connection = fixture
            .service
            .session_connection_info(&fixture.thread)?
            .unwrap();
        assert_ne!(target_connection.id, fixture.connection);
        assert_eq!(
            fixture.service.session_connection_info(&other)?.unwrap().id,
            fixture.connection
        );
        assert_eq!(fixture.service.snapshot(&other)?.status, "working");
        assert!(loaded.recovery.is_none());
        fixture.file("release-prompt", "");
        until(|| fixture.service.snapshot(&other).unwrap().status == "idle").await;
        assert!(fixture.service.snapshot(&other)?.error.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn timed_out_close_quarantines_the_old_route_even_if_close_finishes_later() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    let other = fixture.import("other")?;
    fixture.service.resume(&other).await?;
    fixture.file("hold-load", "");
    fixture.file("hold-close", "");
    let load = fixture.load();
    fixture.event("session/load", "target", "start", 1).await;
    fixture.service.cancel(&fixture.thread)?;
    assert!(load.await?.is_err());
    fixture.file("release-load", "");
    fixture.event("session/close", "target", "start", 1).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::time::resume();
    fixture.settled().await;
    let restored = fixture.service.resume(&fixture.thread).await?;
    fixture.file("release-close", "");
    fixture
        .event("session/close", "target", "complete", 1)
        .await;
    assert_eq!(entries(&fixture.snapshot()), entries(&restored));
    assert!(fixture.snapshot().recovery.is_none());
    Ok(())
}

#[tokio::test]
async fn resume_only_uses_the_same_owned_lifecycle_without_the_short_deadline() -> Result<()> {
    let fixture = Fixture::new(json!({"RESUME_ONLY":"1"})).await?;
    fixture.file("hold-load", "");
    let load = fixture.load();
    fixture.event("session/resume", "target", "start", 1).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::time::resume();
    assert!(!load.is_finished());
    fixture.file("release-load", "");
    let restored = load.await??;
    assert!(restored.recovery.is_none());
    fixture
        .service
        .start_prompt(&fixture.thread, prompt("next"))?;
    until(|| fixture.snapshot().status == "idle").await;
    assert!(fixture.snapshot().error.is_none());
    Ok(())
}

#[tokio::test]
async fn previously_untracked_loaded_session_recovers_without_restarting_shared_adapter()
-> Result<()> {
    let fixture = Fixture::new(json!({"PRELOAD_FIRST":"1"})).await?;
    let error = fixture.service.resume(&fixture.thread).await.unwrap_err();
    assert!(format!("{error:#}").contains("already loaded"));
    fixture.service.resume(&fixture.thread).await?;
    assert_ne!(
        fixture
            .service
            .session_connection_info(&fixture.thread)?
            .unwrap()
            .id,
        fixture.connection
    );
    let events = fixture.events("session/load", "target", "start");
    assert_eq!(events.len(), 2);
    assert_ne!(events[0]["pid"], events[1]["pid"]);
    Ok(())
}

#[tokio::test]
async fn shared_load_error_preserves_authentication_classification() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    fixture.file("fail-load", "auth");
    let error = fixture.service.resume(&fixture.thread).await.unwrap_err();
    assert!(aow_acp::is_auth_required(&error));
    assert!(fixture.snapshot().auth_required);
    assert!(fixture.snapshot().error.is_none());
    fixture.remove("fail-load");
    assert!(!fixture.service.resume(&fixture.thread).await?.auth_required);
    assert_eq!(
        fixture
            .service
            .session_connection_info(&fixture.thread)?
            .unwrap()
            .id,
        fixture.connection
    );
    Ok(())
}

#[tokio::test]
async fn long_replay_installs_two_complete_turns_once_with_protocol_ids_preserved() -> Result<()> {
    let fixture = Fixture::new(json!({"LONG_HISTORY":"1"})).await?;
    let snapshot = fixture.service.resume(&fixture.thread).await?;
    let overview = fixture.service.overview(&fixture.thread)?.overview.unwrap();
    assert_eq!(overview.len(), 2);
    assert_eq!(overview[0].tool_count, 40);
    assert_eq!(overview[1].tool_count, 150);
    assert_eq!(
        overview[1].conclusion[0].message_id.as_deref(),
        Some("final-1")
    );
    assert_eq!(
        snapshot.entries.last().unwrap().content["text"],
        "complete answer"
    );
    assert_eq!(
        snapshot
            .entries
            .iter()
            .filter(|entry| entry.kind == "tool")
            .count(),
        190
    );
    assert_eq!(
        entries(&snapshot),
        entries(&fixture.service.resume(&fixture.thread).await?)
    );
    Ok(())
}

#[tokio::test]
async fn closing_a_pending_load_waits_for_remote_completion_before_close() -> Result<()> {
    let fixture = Fixture::new(json!({})).await?;
    fixture.file("hold-load", "");
    let load = fixture.load();
    fixture.event("session/load", "target", "start", 1).await;
    let close = fixture.service.close_session(&fixture.thread);
    tokio::pin!(close);
    assert!(futures::poll!(&mut close).is_pending());
    assert!(
        fixture
            .events("session/close", "target", "start")
            .is_empty()
    );
    assert!(load.await?.is_err());
    fixture.file("release-load", "");
    close.await?;
    assert!(fixture.snapshot().entries.is_empty());
    fixture.service.resume(&fixture.thread).await?;
    assert_eq!(
        fixture.events("session/close", "target", "complete").len(),
        1
    );
    Ok(())
}
