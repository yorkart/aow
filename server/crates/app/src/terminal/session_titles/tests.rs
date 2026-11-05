use super::*;
#[tokio::test]
async fn metadata_preserves_osc_titles_and_filters_other_workspaces() {
    use super::super::{
        TerminalManager,
        tests::{pane, tab_with},
    };
    use aow_api_contract::terminal::{TerminalLayout, TerminalPaneStatus};
    use aow_runtime_client::TerminalClient;
    use axum::{Json, Router, routing::get};

    let directory = tempfile::tempdir().unwrap();
    let agent = "claude";
    let detected = TerminalAgentList {
        agents: [
            ("one".into(), Some(agent.into())),
            ("unrelated".into(), None),
        ]
        .into(),
        titles: [
            ("one".into(), "[ ! ] Action Required | Native OSC".into()),
            ("unrelated".into(), "Other workspace".into()),
        ]
        .into(),
        ..Default::default()
    };
    let app = Router::new().route(
        "/v1/terminal/agents",
        get(move || {
            let detected = detected.clone();
            async move { Json(detected) }
        }),
    );
    let socket = directory.path().join("daemon.sock");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::fs::write(&socket,serde_json::json!({"url":format!("http://{}",listener.local_addr().unwrap()),"token":"fixture-token-0123456789abcdef0123456789"}).to_string()).unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let manager = TerminalManager::in_memory(TerminalClient::for_test_config(socket));
    manager.lock_state().unwrap().tabs.push(tab_with(
        TerminalLayout::Pane {
            pane_id: "one".into(),
        },
        vec![pane("one", TerminalPaneStatus::Running)],
    ));
    let detected = manager.agents(Some("/tmp")).await.unwrap();
    assert_eq!(detected.titles.len(), 1);
    assert_eq!(detected.titles["one"], "[ ! ] Action Required | Native OSC");
    assert!(
        manager
            .agents(Some("/another-workspace"))
            .await
            .unwrap()
            .titles
            .is_empty()
    );
    server.abort();
}
