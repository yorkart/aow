use super::*;
use crate::sessions::{AgentSession, AgentSessionLocator};
struct EmptyTerminals;
#[async_trait::async_trait]
impl ManagedTerminals for EmptyTerminals {
    async fn agents(&self) -> Result<aow_protocol::TerminalAgentList> {
        unreachable!("catalog-only tests do not inspect terminals")
    }
    fn runtimes(&self) -> Result<aow_protocol::TerminalRuntimeList> {
        unreachable!("catalog-only tests do not inspect terminals")
    }
}
use chrono::Utc;

#[tokio::test]
async fn opaque_handles_survive_restart_without_server_transcript_paths() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let cwd = root.path().join("removed-worktree");
    std::fs::create_dir(&cwd).unwrap();
    let transcript = home.path().join("session.jsonl");
    std::fs::write(&transcript,"{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"hello from node\"}]}}\n").unwrap();
    let service =
        SessionCatalog::open(root.path(), "node-a".into(), Arc::new(EmptyTerminals)).unwrap();
    let mut locator = AgentSessionLocator {
        agent: "codex",
        session_id: "native-id".into(),
        title: "First".into(),
        cwd: cwd.clone(),
        transcript_path: transcript.clone(),
        trusted_root: home.path().into(),
    };
    let make = |locator: AgentSessionLocator| AgentSession::new(locator, Utc::now(), Utc::now());
    let value = service.remember(&make(locator.clone())).unwrap();
    assert!(!value.to_string().contains(transcript.to_str().unwrap()));
    let reference: SessionReference = serde_json::from_value(value["reference"].clone()).unwrap();
    locator.title = "Renamed".into();
    let renamed = service.remember(&make(locator.clone())).unwrap();
    assert_eq!(renamed["reference"]["handle"], reference.handle);
    drop(service);
    std::fs::remove_dir(cwd).unwrap();
    let service =
        SessionCatalog::open(root.path(), "node-a".into(), Arc::new(EmptyTerminals)).unwrap();
    let found = service.locate(&reference).unwrap();
    assert_eq!(found.title, "Renamed");
    let snapshot = serde_json::to_value(crate::sessions::snapshot::read(found).unwrap()).unwrap();
    assert!(snapshot.to_string().contains("hello from node"));
    let mut invalid = reference.clone();
    invalid.node_id = "node-b".into();
    assert!(service.locate(&invalid).is_err());
    invalid = reference.clone();
    invalid.cwd = "/another-worktree".into();
    assert!(service.locate(&invalid).is_err());
    invalid = reference.clone();
    invalid.session_id = "other-id".into();
    assert!(service.locate(&invalid).is_err());
    locator.transcript_path = home.path().join("other.jsonl");
    let different = service.remember(&make(locator)).unwrap();
    assert_ne!(different["reference"]["handle"], reference.handle);
}

#[tokio::test]
async fn persisted_reference_still_checks_trusted_history_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("escaped.jsonl");
    std::fs::write(&path, "").unwrap();
    let service =
        SessionCatalog::open(root.path(), "node".into(), Arc::new(EmptyTerminals)).unwrap();
    let session = AgentSession::new(
        AgentSessionLocator {
            agent: "codex",
            session_id: "id".into(),
            title: "".into(),
            cwd: root.path().into(),
            transcript_path: path,
            trusted_root: root.path().into(),
        },
        Utc::now(),
        Utc::now(),
    );
    let reference: SessionReference =
        serde_json::from_value(service.remember(&session).unwrap()["reference"].clone()).unwrap();
    assert!(crate::sessions::snapshot::read(service.locate(&reference).unwrap()).is_err());
}
