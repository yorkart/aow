use std::{fs::OpenOptions, io::Write, time::Duration};
#[cfg(unix)]
#[tokio::test]
async fn collector_reads_the_managed_process_store_and_keeps_its_final_completion() {
    use aow_agent_runtime_core::catalog::SessionCatalog;
    use aow_terminal_core::TerminalRuntimeHost;
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("project");
    std::fs::create_dir(&cwd).unwrap();
    let home = root.path().join("native-home");
    let store = home.join("sessions");
    std::fs::create_dir_all(&store).unwrap();
    let transcript = store.join("native.jsonl");
    std::fs::write(&transcript,"{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"historic\"}}\n").unwrap();
    let db = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!(
        "../../../agent-runtime-core/src/completions/sql/tests/create_session_threads.sql"
    ))
    .unwrap();
    db.execute(
        include_str!(
            "../../../agent-runtime-core/src/completions/sql/tests/insert_session_thread.sql"
        ),
        rusqlite::params!["native", transcript.to_str(), cwd.to_str()],
    )
    .unwrap();
    // A copied native binary supplies a real process identity without requiring an installed agent.
    let executable = root.path().join("traecli");
    std::fs::copy("/bin/cat", &executable).unwrap();
    let mut host = TerminalRuntimeHost::start(None).await;
    let sessions = SessionCatalog::open(
        root.path(),
        "node".into(),
        std::sync::Arc::new(host.observer()),
    )
    .unwrap();
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let service = aow_agent_runtime_core::completions::Completions::open(
        root.path(),
        "node".into(),
        stopping.clone(),
    )
    .unwrap();
    let collector = service.start(sessions.clone(), stopping);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router =
        axum::Router::new().nest("/v1/terminal", aow_terminal_service::router(host.manager()));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::new();
    let url = format!("http://{address}/v1/terminal/runtimes/pane");
    client.put(&url).json(&json!({"cwd":cwd,"shell":"/bin/sh","arguments":["-c",r#"printf '\033]0;Same title\007'; exec "$FIXTURE_CLI""#],"environment":{"TRAECLI_HOME":home,"FIXTURE_CLI":executable},"rows":24,"cols":80})).send().await.unwrap().error_for_status().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let binding = aow_agent_runtime_core::catalog::read_pane(
                sessions.clone(),
                aow_host_contract::agents::NativePaneRequest {
                    pane_id: "pane".into(),
                },
            )
            .await
            .ok();
            if binding.as_ref().is_some_and(|binding| {
                binding["agent"] == "traecli" && !binding["current"].is_null()
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    // Repeated fresh records avoid timing assumptions about the first EOF registration.
    tokio::time::timeout(Duration::from_secs(10),async{
        loop {
            writeln!(OpenOptions::new().append(true).open(&transcript).unwrap(),"{}",json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"probe","last_agent_message":"node-only conclusion"}})).unwrap();
            if service.bounds().unwrap().1>0{break;}
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.unwrap();
    let events = service.read(0).unwrap();
    assert!(
        events
            .iter()
            .all(|event| event["event"]["turn_id"] != "historic")
    );
    assert_eq!(events[0]["event"]["agent"], "traecli");
    assert_eq!(events[0]["event"]["instance_ids"], json!(["pane"]));
    db.execute(
        include_str!("../../../agent-runtime-core/src/completions/sql/tests/rename_thread.sql"),
        rusqlite::params!["Renamed on node", "native"],
    )
    .unwrap();
    writeln!(
        OpenOptions::new().append(true).open(&transcript).unwrap(),
        "{}",
        json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"last"}})
    )
    .unwrap();
    // Remove the PTY before the next poll: tails must be read before releasing bindings.
    client
        .delete(&url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = service.read(0).unwrap();
            if let Some(event) = events
                .iter()
                .find(|event| event["event"]["turn_id"] == "last")
            {
                assert_eq!(event["event"]["title"], "Renamed on node");
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    stop.send_replace(true);
    collector.await.unwrap();
    host.shutdown().await.unwrap();
    server.abort();
}
