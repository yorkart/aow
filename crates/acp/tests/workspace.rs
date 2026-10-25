use anyhow::Result;
use aow_acp::{AcpHost, AcpService, Content};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Host {
    log: PathBuf,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(json!({"agent_servers":{"test":{"type":"custom","command":"node",
            "args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace-agent.mjs")],
            "env":{"ACP_WORKSPACE_RPC_LOG":self.log}}}}).to_string())
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
async fn workspace_cleanup_removes_local_history_without_deleting_native_sessions() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let target = root.join("worktree");
    let other = root.join("worktree-other");
    let nested = target.join("nested");
    std::fs::create_dir_all(&nested)?;
    std::fs::create_dir(&other)?;
    let log = root.join("rpc.log");
    let storage = root.join("acp");
    let host = Arc::new(Host { log: log.clone() });
    let service = AcpService::new(host.clone(), Some(storage.clone()))?;
    let connection = service.connect("test", target.clone()).await?;
    let active = service.new_session(&connection.id).await?;
    service.start_prompt(
        &active.thread_id,
        serde_json::from_value::<Vec<Content>>(json!([{"type":"text","text":"wait"}]))?,
    )?;
    let offline = service.prepare_session("test", target.clone()).await?;
    let other_connection = service.connect("test", other.clone()).await?;
    let kept = service.new_session(&other_connection.id).await?;
    let child = service.prepare_session("test", nested.clone()).await?;
    let target = target.to_str().unwrap();
    service.set_workspace_removing(target, true);
    assert!(
        service
            .prepare_session("test", target.into())
            .await
            .is_err()
    );
    assert!(service.resume(&active.thread_id).await.is_err());
    let sqlite = rusqlite::Connection::open(storage.join("threads.sqlite"))?;
    sqlite.execute_batch(include_str!("sql/reject_workspace_delete.sql"))?;
    assert!(service.delete_workspace(target).await.is_err());
    assert_eq!(
        service.sessions(Some(target)).len(),
        2,
        "failed deletes must remain retryable"
    );
    sqlite.execute_batch(include_str!("sql/allow_workspace_delete.sql"))?;
    assert_eq!(service.delete_workspace(target).await?, 2);
    service.set_workspace_removing(target, false);
    assert!(service.sessions(Some(target)).is_empty());
    assert!(service.snapshot(&active.thread_id).is_err());
    assert!(service.snapshot(&offline.thread_id).is_err());
    assert!(service.logs(&connection.id).unwrap_or_default().is_empty());
    assert!(service.connection_requests(&connection.id).is_err());
    assert!(service.snapshot(&kept.thread_id).is_ok());
    assert!(service.session_connection_info(&kept.thread_id)?.is_some());
    assert!(service.snapshot(&child.thread_id).is_ok());
    let db = aow_db::acp::AcpDb::open(Some(&storage.join("threads.sqlite")))?;
    assert!(db.load_snapshot(&active.thread_id)?.is_none());
    assert!(db.load_snapshot(&offline.thread_id)?.is_none());
    assert_eq!(db.list_threads()?.len(), 2);
    assert!(
        !std::fs::read_to_string(log)?
            .lines()
            .any(|method| method == "session/delete")
    );
    service.set_workspace_removing(target, true);
    assert_eq!(service.delete_workspace(target).await?, 0);
    service.set_workspace_removing(target, false);
    drop(service);
    let restored = AcpService::new(host, Some(storage))?;
    assert!(restored.sessions(Some(target)).is_empty());
    assert_eq!(restored.sessions(None).len(), 2);
    // A subsequent worktree using the same path can create a fresh local instance.
    assert!(
        restored
            .prepare_session("test", target.into())
            .await
            .is_ok()
    );
    Ok(())
}

struct DelayedHost {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    host: Host,
}
#[async_trait::async_trait]
impl AcpHost for DelayedHost {
    async fn settings(&self) -> Result<String> {
        self.entered.notify_one();
        self.release.notified().await;
        self.host.settings().await
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
async fn pending_preparation_cannot_recreate_history_after_workspace_cleanup() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let cwd = directory.path().canonicalize()?;
    let host = Arc::new(DelayedHost {
        entered: Default::default(),
        release: Default::default(),
        host: Host {
            log: cwd.join("rpc.log"),
        },
    });
    let service = AcpService::new(host.clone(), None)?;
    let pending_service = service.clone();
    let pending_cwd = cwd.clone();
    let pending =
        tokio::spawn(async move { pending_service.prepare_session("test", pending_cwd).await });
    host.entered.notified().await;
    let workspace = cwd.to_str().unwrap();
    service.set_workspace_removing(workspace, true);
    assert_eq!(service.delete_workspace(workspace).await?, 0);
    service.set_workspace_removing(workspace, false);
    host.release.notify_one();
    assert!(pending.await?.is_err());
    assert!(service.sessions(None).is_empty());
    Ok(())
}

#[tokio::test]
async fn failed_delete_must_not_run_later_without_retry() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let storage = root.join("acp");
    let service = AcpService::new(
        Arc::new(Host {
            log: root.join("rpc.log"),
        }),
        Some(storage.clone()),
    )?;
    let session = service.prepare_session("test", root.clone()).await?;
    let sqlite = rusqlite::Connection::open(storage.join("threads.sqlite"))?;
    sqlite.execute_batch(include_str!("sql/reject_workspace_delete.sql"))?;
    let cwd = root.to_str().unwrap();
    service.set_workspace_removing(cwd, true);
    assert!(service.delete_workspace(cwd).await.is_err());
    service.set_workspace_removing(cwd, false);
    sqlite.execute_batch(include_str!("sql/allow_workspace_delete.sql"))?;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(service.sessions(Some(cwd)).len(), 1);
    let db = aow_db::acp::AcpDb::open(Some(&storage.join("threads.sqlite")))?;
    assert!(
        db.load_snapshot(&session.thread_id)?.is_some(),
        "database row disappeared after cleanup reported failure, without an explicit retry"
    );
    Ok(())
}

#[tokio::test]
async fn queued_removal_must_not_lose_an_existing_reply() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().canonicalize()?;
    let service = AcpService::new(
        Arc::new(Host {
            log: root.join("rpc.log"),
        }),
        None,
    )?;
    let connection = service.connect("test", root.clone()).await?;
    let session = service.new_session(&connection.id).await?;
    service.start_prompt(
        &session.thread_id,
        serde_json::from_value::<Vec<Content>>(json!([{"type":"text","text":"hello"}]))?,
    )?;
    let cwd = root.to_str().unwrap();
    service.set_workspace_removing(cwd, true);
    assert!(service.resume(&session.thread_id).await.is_err());
    assert!(
        service
            .start_prompt(
                &session.thread_id,
                serde_json::from_value::<Vec<Content>>(
                    json!([{"type":"text","text":"new prompt"}])
                )?
            )
            .is_err()
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while service.snapshot(&session.thread_id).unwrap().status != "idle" {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    // Inspection rejects deletion, e.g. because the worktree has uncommitted files.
    service.set_workspace_removing(cwd, false);
    let current = service.snapshot(&session.thread_id)?;
    assert_eq!(
        current.status, "idle",
        "completion was discarded while removal was queued"
    );
    assert!(
        current
            .entries
            .iter()
            .any(|entry| entry.kind == "assistant")
    );
    Ok(())
}
