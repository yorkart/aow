use anyhow::Result;
use aow_acp::{AcpHost, AcpService, Content, SessionSnapshot};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Host {
    supported: bool,
    completions: std::sync::Mutex<Vec<aow_acp::TurnCompletion>>,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn turn_completed(&self, completion: aow_acp::TurnCompletion) {
        self.completions.lock().unwrap().push(completion);
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(None)
    }
    async fn read_text_file(&self, _: &std::path::Path, _: &std::path::Path) -> Result<String> {
        anyhow::bail!("fixture does not read files")
    }
    async fn write_text_file(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
        _: &str,
    ) -> Result<()> {
        anyhow::bail!("fixture does not write files")
    }
    async fn settings(&self) -> Result<String> {
        Ok(json!({"agent_servers":{"generic":{"type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steering-agent.mjs")],"env":{"STEERING":if self.supported {"1"} else {"0"}}}}}).to_string())
    }
}

fn prompt(text: &str) -> Vec<Content> {
    serde_json::from_value(json!([{"type":"text","text":text}])).unwrap()
}

struct Fixture {
    host: Arc<Host>,
    service: AcpService,
    directory: tempfile::TempDir,
    id: String,
}
impl Fixture {
    async fn new(supported: bool) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let host = Arc::new(Host {
            supported,
            completions: Default::default(),
        });
        let service = AcpService::new(host.clone(), Some(directory.path().join("state")))?;
        let connection = service.connect("generic", directory.path().into()).await?;
        assert_eq!(connection.extensions["steering"]["supported"], supported);
        let thread = service.new_session(&connection.id).await?;
        Ok(Self {
            host,
            service,
            directory,
            id: thread.thread_id,
        })
    }
    async fn wait(&self, condition: impl Fn(&SessionSnapshot) -> bool) -> SessionSnapshot {
        let mut events = self.service.subscribe_sync();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = self.service.snapshot(&self.id).unwrap();
                if condition(&snapshot) {
                    return snapshot;
                }
                let _ = events.recv().await;
            }
        })
        .await
        .expect("ACP fixture did not reach expected state")
    }
    async fn start(&self, text: &str) -> Result<()> {
        self.service.start_prompt(&self.id, prompt(text))?;
        self.wait(|s| {
            s.entries
                .iter()
                .any(|entry| entry.content["text"] == "Before steering")
        })
        .await;
        Ok(())
    }
    fn wire(&self, method: &str) -> Vec<Value> {
        std::fs::read_to_string(self.directory.path().join("steering-wire.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|item| item["method"] == method)
            .collect()
    }
}

#[tokio::test]
async fn generic_steering_preserves_the_turn_permissions_and_echoes() -> Result<()> {
    let f = Fixture::new(true).await?;
    f.start("permission").await?;
    f.wait(|s| !s.permissions.is_empty()).await;
    for text in ["First change", "Second change"] {
        f.service.start_steering(&f.id, prompt(text))?;
        let s = f.wait(|s| !s.steering_pending).await;
        assert_eq!(s.status, "working");
        assert_eq!(s.turns.len(), 1);
        assert_eq!(s.turns[0].status, "in_progress");
        assert_eq!(s.permissions.len(), 1);
        assert_eq!(
            s.entries
                .iter()
                .filter(|entry| entry.content["text"] == text)
                .count(),
            1
        );
        assert_eq!(
            f.wire("_session/steering").last().unwrap()["params"]["prompt"],
            serde_json::json!([{"type": "text", "text": text}])
        );
        assert!(
            s.entries
                .iter()
                .filter_map(|entry| entry.delivery.as_ref())
                .all(|d| d.status == "injected")
        );
        assert_eq!(f.service.overview(&f.id)?.overview.unwrap().len(), 1);
    }
    assert_eq!(f.wire("session/prompt").len(), 1);
    assert!(f.wire("session/cancel").is_empty());
    assert_eq!(
        f.wire("_session/steering")[0]["params"]["_meta"]["steering"]["idleBehavior"],
        "promptRequired"
    );
    f.service.cancel(&f.id)?;
    assert_eq!(
        f.wait(|s| s.status == "idle").await.turns[0].status,
        "interrupted"
    );
    Ok(())
}

#[tokio::test]
async fn steering_checks_capability_and_rejects_concurrent_submissions() -> Result<()> {
    let unsupported = Fixture::new(false).await?;
    unsupported.start("wait").await?;
    assert!(
        unsupported
            .service
            .start_steering(&unsupported.id, prompt("change"))
            .is_err()
    );
    assert!(unsupported.wire("_session/steering").is_empty());
    let f = Fixture::new(true).await?;
    f.start("wait").await?;
    f.service.start_steering(&f.id, prompt("hold"))?;
    assert!(
        f.service
            .start_steering(&f.id, prompt("duplicate"))
            .is_err()
    );
    f.service.cancel(&f.id)?;
    let s = f.wait(|s| s.status == "idle" && !s.steering_pending).await;
    assert_eq!(
        s.entries
            .iter()
            .find_map(|entry| entry.delivery.as_ref())
            .unwrap()
            .status,
        "cancelled"
    );
    assert_eq!(
        f.wire("session/prompt").len(),
        1,
        "cancel must prevent idle fallback"
    );
    Ok(())
}

#[tokio::test]
async fn idle_races_do_not_duplicate_input_or_lose_prompt_ownership() -> Result<()> {
    for text in ["finish-before-ack", "prompt-required", "fast-new-turn"] {
        let f = Fixture::new(true).await?;
        f.start("wait").await?;
        f.service.start_steering(&f.id, prompt(text))?;
        let s = f.wait(|s| s.status == "idle" && !s.steering_pending).await;
        assert_eq!(
            s.entries
                .iter()
                .filter(|entry| entry.content["text"] == text)
                .count(),
            1,
            "{text}"
        );
        assert!(
            s.entries
                .iter()
                .filter_map(|entry| entry.delivery.as_ref())
                .all(|d| d.status != "sending")
        );
        assert_eq!(
            f.wire("session/prompt").len(),
            if text == "prompt-required" { 2 } else { 1 },
            "{text}"
        );
        assert_eq!(
            s.turns.len(),
            if text == "finish-before-ack" { 1 } else { 2 }
        );
        assert_eq!(
            s.turns.last().unwrap().status,
            if text == "fast-new-turn" {
                "unknown"
            } else {
                "completed"
            }
        );
    }
    Ok(())
}

#[tokio::test]
async fn detached_turn_stays_working_after_old_prompt_completes_and_can_be_cancelled() -> Result<()>
{
    let f = Fixture::new(true).await?;
    f.start("wait").await?;
    f.service.start_steering(&f.id, prompt("new-turn"))?;
    let s = f
        .wait(|s| !s.steering_pending && s.turns.len() == 2 && s.turns[0].status == "completed")
        .await;
    assert_eq!(s.status, "working");
    assert_eq!(s.turns[1].status, "in_progress");
    f.service.cancel(&f.id)?;
    let s = f.wait(|s| s.status == "idle").await;
    assert_eq!(s.turns[1].status, "interrupted");
    assert_eq!(f.wire("session/prompt").len(), 1);
    Ok(())
}

#[tokio::test]
async fn failed_and_unknown_delivery_are_recorded_without_resending_or_stopping() -> Result<()> {
    for (text, expected) in [("failed", "failed"), ("unexpected", "unknown")] {
        let f = Fixture::new(true).await?;
        f.start("wait").await?;
        f.service.start_steering(&f.id, prompt(text))?;
        let s = f.wait(|s| !s.steering_pending).await;
        assert_eq!(
            s.entries
                .iter()
                .find_map(|entry| entry.delivery.as_ref())
                .unwrap()
                .status,
            expected
        );
        assert_eq!(s.status, "working");
        assert_eq!(s.turns.len(), 1);
        assert!(f.wire("session/cancel").is_empty());
        assert_eq!(f.wire("session/prompt").len(), 1);
    }
    Ok(())
}

#[tokio::test]
async fn completion_is_emitted_once_even_when_the_prompt_settles_before_steer_ack() -> Result<()> {
    let f = Fixture::new(true).await?;
    f.start("wait").await?;
    f.service
        .start_steering(&f.id, prompt("finish-before-ack"))?;
    f.wait(|s| s.status == "idle" && !s.steering_pending).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.host.completions.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let completions = f.host.completions.lock().unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].conclusion.as_deref(), Some("After steering"));
    Ok(())
}

#[tokio::test]
async fn idle_before_admission_tracks_new_activity_on_either_side_of_ack() -> Result<()> {
    for text in ["fast-new-turn", "late-new-turn"] {
        let f = Fixture::new(true).await?;
        f.start("early-idle").await?;
        f.service.start_steering(&f.id, prompt(text))?;
        let s = f.wait(|s| s.status == "idle" && !s.steering_pending).await;
        assert!(!s.steering_untracked);
        assert_eq!(s.turns.len(), 2);
        assert_eq!(s.turns[0].status, "completed");
        assert_eq!(s.turns[1].status, "unknown");
        assert_eq!(f.wire("session/prompt").len(), 1);
        assert!(f.wire("session/cancel").is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn session_load_preserves_steering_across_consumption_order_and_chunk_changes() -> Result<()>
{
    let f = Fixture::new(true).await?;
    f.start("replay").await?;
    f.wait(|s| {
        s.entries
            .iter()
            .any(|entry| entry.content["text"] == "Answer ")
    })
    .await;
    f.service.start_steering(&f.id, prompt("Correction"))?;
    f.wait(|s| s.status == "idle" && !s.steering_pending).await;
    f.service.close_session(&f.id).await?;
    let after = f.service.resume(&f.id).await?;
    let steering: Vec<_> = after
        .entries
        .iter()
        .filter(|entry| entry.kind == "steering")
        .collect();
    assert_eq!(steering.len(), 1);
    assert_eq!(steering[0].delivery.as_ref().unwrap().status, "injected");
    assert_eq!(after.turns.len(), 1);
    assert_eq!(after.turns[0].status, "completed");
    assert_eq!(f.service.overview(&f.id)?.overview.unwrap().len(), 1);
    assert_eq!(f.wire("session/load").len(), 1);
    Ok(())
}
