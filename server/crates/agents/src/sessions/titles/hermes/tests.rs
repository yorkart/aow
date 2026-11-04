use super::*;
use crate::sessions::SessionEnvironment;
use rusqlite::{Connection, params};
use serde_json::json;

struct Fixture {
    connection: Connection,
    directory: tempfile::TempDir,
    environment: SessionEnvironment,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let connection = Connection::open(root.join("state.db")).unwrap();
        connection
            .execute_batch(include_str!("sql/create_sessions.sql"))
            .unwrap();
        std::fs::create_dir(root.join("runtime")).unwrap();
        Self {
            connection,
            directory,
            environment: [("HOME".into(), root.clone()), ("HERMES_HOME".into(), root)].into(),
        }
    }

    fn context(&self, pid: i32) -> LiveSessionContext<'_> {
        LiveSessionContext {
            pid: Some(pid),
            cwd: "/workspace",
            title: "unrelated shell title",
            environment: &self.environment,
        }
    }

    fn session(&self, id: &str, title: Option<&str>, prompt: &str) {
        self.connection
            .execute(include_str!("sql/insert_session.sql"), params![id, title])
            .unwrap();
        self.connection
            .execute(include_str!("sql/insert_message.sql"), params![id, prompt])
            .unwrap();
    }

    fn leases(&self, entries: &[(i32, &str)]) {
        let entries: Vec<_> = entries
            .iter()
            .map(|(pid, id)| json!({"pid":pid,"surface":"cli","session_id":id}))
            .collect();
        std::fs::write(
            self.directory.path().join("runtime/active_sessions.json"),
            json!({"entries": entries}).to_string(),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn native_titles_refresh_without_completion_and_isolate_same_cwd_processes() {
    let fixture = Fixture::new();
    let provider = Agent::Hermes.session_titles().unwrap();
    fixture.session("first", None, "请修复\n登录接口");
    fixture.session("second", Some("另一个会话"), "Other prompt");
    fixture.leases(&[(101, "first"), (202, "second")]);
    assert_eq!(
        provider
            .session_title(fixture.context(101))
            .await
            .as_deref(),
        Some("请修复 登录接口")
    );
    assert_eq!(
        provider
            .session_title(fixture.context(202))
            .await
            .as_deref(),
        Some("另一个会话")
    );

    fixture
        .connection
        .execute(include_str!("sql/rename_session.sql"), [])
        .unwrap();
    assert_eq!(
        provider
            .session_title(fixture.context(101))
            .await
            .as_deref(),
        Some("登录接口修复")
    );
    assert_eq!(
        provider
            .session_title(fixture.context(202))
            .await
            .as_deref(),
        Some("另一个会话")
    );

    // /new can publish a lease before the first native transcript exists.
    fixture.leases(&[(101, "new"), (202, "second")]);
    assert!(provider.session_title(fixture.context(101)).await.is_none());
    fixture.session("new", Some("新的任务"), "New prompt");
    assert_eq!(
        provider
            .session_title(fixture.context(101))
            .await
            .as_deref(),
        Some("新的任务")
    );
}

#[tokio::test]
async fn native_titles_require_a_valid_unique_lease_in_the_process_profile_and_cwd() {
    let fixture = Fixture::new();
    fixture.session("session", Some("Native title"), "Prompt");
    assert!(Hermes.session_title(fixture.context(101)).await.is_none());
    fixture.leases(&[(101, "session")]);
    assert!(Hermes.session_title(fixture.context(202)).await.is_none());
    assert!(
        Hermes
            .session_title(LiveSessionContext {
                cwd: "/elsewhere",
                ..fixture.context(101)
            })
            .await
            .is_none()
    );
    let other = Fixture::new();
    assert!(Hermes.session_title(other.context(101)).await.is_none());

    fixture.session("another", Some("Wrong title"), "Prompt");
    fixture.leases(&[(101, "session"), (101, "another")]);
    assert!(Hermes.session_title(fixture.context(101)).await.is_none());
    std::fs::write(
        fixture
            .directory
            .path()
            .join("runtime/active_sessions.json"),
        "broken",
    )
    .unwrap();
    assert!(Hermes.session_title(fixture.context(101)).await.is_none());
    fixture.leases(&[(101, "session")]);
    fixture
        .connection
        .execute(include_str!("sql/end_session.sql"), [])
        .unwrap();
    assert!(Hermes.session_title(fixture.context(101)).await.is_none());
}

#[test]
fn osc_title_agents_do_not_opt_into_native_title_replacement() {
    for agent in [Agent::Claude, Agent::Codex, Agent::TraeCli, Agent::Gemini] {
        assert!(agent.session_titles().is_none());
    }
}
