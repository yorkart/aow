use anyhow::{Result, ensure};
use aow_acp::{AcpHost, AcpService, Content};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Host {
    reject: AtomicBool,
    bindings: Mutex<Vec<(String, String, String)>>,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    fn session_bound(&self, thread: &str, runtime: &str, session: &str) -> Result<()> {
        ensure!(
            !self.reject.load(Ordering::SeqCst),
            "Task binding storage unavailable"
        );
        self.bindings
            .lock()
            .unwrap()
            .push((thread.into(), runtime.into(), session.into()));
        Ok(())
    }
    async fn settings(&self) -> Result<String> {
        Ok(json!({"agent_servers":{"test":{"type":"custom","command":"node","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lineage-agent.mjs")]}}}).to_string())
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
#[tokio::test]
async fn native_binding_precedes_execution_and_is_refreshed_on_resume() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let host = Arc::new(Host {
        reject: AtomicBool::new(false),
        bindings: Mutex::new(Vec::new()),
    });
    let service = AcpService::new(host.clone(), Some(directory.path().join("state")))?;
    let prepared = service
        .prepare_session("test", directory.path().into())
        .await?;
    assert!(prepared.session_id.is_none());
    let ready = service.resume(&prepared.thread_id).await?;
    let native = ready.session_id.as_ref().unwrap();
    assert_eq!(
        host.bindings.lock().unwrap().last().unwrap(),
        &(prepared.thread_id.clone(), "test".into(), native.clone())
    );
    let prompt = || {
        vec![Content {
            kind: "text".into(),
            data: serde_json::Map::from_iter([("text".into(), "Inspect".into())]),
        }]
    };
    host.reject.store(true, Ordering::SeqCst);
    assert!(
        service
            .start_prompt(&ready.thread_id, prompt())
            .unwrap_err()
            .to_string()
            .contains("binding storage unavailable")
    );
    assert!(!directory.path().join("lineage-prompts.jsonl").exists());
    host.reject.store(false, Ordering::SeqCst);
    service.start_prompt(&ready.thread_id, prompt())?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !directory.path().join("lineage-prompts.jsonl").exists()
            || service.snapshot(&ready.thread_id).unwrap().status == "working"
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let before = host.bindings.lock().unwrap().len();
    service.close_session(&ready.thread_id).await?;
    let restored = service.resume(&ready.thread_id).await?;
    assert_eq!(restored.session_id, ready.session_id);
    assert!(host.bindings.lock().unwrap().len() > before);
    service
        .delete_workspace(&directory.path().to_string_lossy())
        .await?;
    Ok(())
}
