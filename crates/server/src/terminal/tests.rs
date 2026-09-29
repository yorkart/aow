use super::*;
use super::{
    bridge::{bounded_sync_runtime, send_bridge_message},
    clipboard::clipboard_image_type,
};
use axum::{body::to_bytes, http::Request};
use bytes::Bytes;
use futures_util::Sink;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::SystemTime,
};
use tempfile::TempDir;
use tokio::sync::{mpsc, oneshot};

fn minimal_png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13_u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(b"IDAT");
    bytes.push(0);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes.extend_from_slice(b"IEND");
    bytes.extend_from_slice(&[0; 4]);
    bytes
}

fn minimal_jpeg() -> Vec<u8> {
    b"\xff\xd8\xff\xc0\x00\x0b\x08\x00\x01\x00\x01\x01\x01\x11\x00\xff\xda\x00\x08\x01\x01\x00\x00\x3f\x00\x00\xff\xd9".to_vec()
}

fn minimal_gif() -> Vec<u8> {
    b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x01L\x00;".to_vec()
}

fn minimal_webp() -> Vec<u8> {
    b"RIFF\x10\x00\x00\x00WEBPVP8 \x04\x00\x00\x00data".to_vec()
}

fn extended_webp() -> Vec<u8> {
    let mut bytes = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
    bytes.extend_from_slice(b"VP8X\x0a\x00\x00\x00");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(b"EXIF\x03\x00\x00\x00abc\x00");
    bytes.extend_from_slice(b"VP8 \x04\x00\x00\x00data");
    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    bytes
}
use tower::ServiceExt;

pub(super) fn pane(id: &str, status: TerminalPaneStatus) -> TerminalPane {
    TerminalPane {
        id: id.to_owned(),
        name: "tmp".to_owned(),
        cwd: "/tmp".to_owned(),
        shell: "/bin/sh".to_owned(),
        arguments: Vec::new(),
        kind: TerminalPaneKind::Terminal,
        agent_id: None,
        agent_profile_id: None,
        agent_terminal: None,
        restart_on_daemon_restart: true,
        status,
        rows: DEFAULT_ROWS,
        cols: DEFAULT_COLS,
        exit_code: None,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
    }
}

pub(super) fn tab_with(layout: TerminalLayout, panes: Vec<TerminalPane>) -> TerminalTab {
    TerminalTab {
        id: "tab-1".to_owned(),
        name: "Terminal".to_owned(),
        name_is_custom: None,
        workspace_root: "/tmp".to_owned(),
        layout,
        panes,
        revision: 7,
        created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        updated_at: "2026-01-01T00:00:00.000Z".to_owned(),
    }
}

fn tab_for(id: &str, workspace_root: &str) -> TerminalTab {
    let pane_id = format!("{id}-pane");
    let mut tab = tab_with(
        TerminalLayout::Pane {
            pane_id: pane_id.clone(),
        },
        vec![pane(&pane_id, TerminalPaneStatus::Interrupted)],
    );
    tab.id = id.to_owned();
    tab.name = id.to_owned();
    tab.workspace_root = workspace_root.to_owned();
    tab
}

struct PendingSink;

impl Sink<()> for PendingSink {
    type Error = std::convert::Infallible;

    fn poll_ready(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }

    fn start_send(self: Pin<&mut Self>, _item: ()) -> Result<(), Self::Error> {
        unreachable!("a permanently pending sink is never ready")
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }
}

#[tokio::test]
async fn bridge_message_send_is_bounded() {
    let mut sink = PendingSink;
    let result = send_bridge_message(&mut sink, (), Duration::from_millis(1)).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn bridge_runtime_sync_is_bounded_while_waiting_for_sync_lock() {
    let manager = manager_with_pane();
    let _sync = manager.inner.runtime_sync.lock().await;
    let result = bounded_sync_runtime(&manager, "pane-1", Duration::from_millis(1)).await;
    assert!(result.is_err());
}

#[test]
fn recursive_layout_replace_remove_and_validation() {
    let mut layout = TerminalLayout::Pane {
        pane_id: "a".into(),
    };
    let split = TerminalLayout::Split {
        axis: TerminalSplitAxis::Row,
        ratio: 0.4,
        first: Box::new(TerminalLayout::Pane {
            pane_id: "a".into(),
        }),
        second: Box::new(TerminalLayout::Pane {
            pane_id: "b".into(),
        }),
    };
    assert!(replace_layout_leaf(&mut layout, "a", &split));
    validate_layout(
        &layout,
        &[
            pane("a", TerminalPaneStatus::Running),
            pane("b", TerminalPaneStatus::Running),
        ],
    )
    .unwrap();
    assert_eq!(
        remove_layout_leaf(layout, "a"),
        Some(TerminalLayout::Pane {
            pane_id: "b".into()
        })
    );
}

#[test]
fn persistent_load_preserves_running_desired_state() {
    let directory = TempDir::new().unwrap();
    let state_path = directory.path().join(METADATA_FILE);
    let mut original = tab_with(
        TerminalLayout::Pane {
            pane_id: "running".into(),
        },
        vec![pane("running", TerminalPaneStatus::Running)],
    );
    // Current metadata includes naming intent; legacy migration has its own test.
    original.name_is_custom = Some(true);
    atomic_save(
        &state_path,
        &PersistedState {
            version: METADATA_VERSION,
            tabs: vec![original.clone()],
        },
    )
    .unwrap();
    let manager = TerminalManager::persistent(
        directory.path().to_path_buf(),
        TerminaldClient::new(directory.path().join("missing.sock")),
    )
    .unwrap();
    assert_eq!(manager.get_snapshot("tab-1").unwrap(), original);
    assert_eq!(
        std::fs::read(state_path).unwrap(),
        serde_json::to_vec_pretty(&PersistedState {
            version: METADATA_VERSION,
            tabs: vec![original]
        })
        .unwrap()
        .into_iter()
        .chain([b'\n'])
        .collect::<Vec<_>>()
    );
}

#[test]
fn terminal_reorder_persists_within_workspace_and_preserves_other_workspaces() {
    let directory = TempDir::new().unwrap();
    let manager = TerminalManager::persistent(
        directory.path().to_path_buf(),
        TerminaldClient::new(directory.path().join("missing.sock")),
    )
    .unwrap();
    manager.lock_state().unwrap().tabs = vec![
        tab_for("a", "/workspace/one"),
        tab_for("x", "/workspace/two"),
        tab_for("b", "/workspace/one"),
        tab_for("y", "/workspace/two"),
        tab_for("c", "/workspace/one"),
    ];
    manager
        .persist_locked(&manager.lock_state().unwrap())
        .unwrap();

    let reordered = manager
        .reorder(ReorderTerminalsRequest {
            workspace_root: "/workspace/one".to_owned(),
            tab_ids: vec!["c".to_owned(), "a".to_owned(), "b".to_owned()],
        })
        .unwrap();
    assert_eq!(
        reordered
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "a", "b"]
    );
    assert_eq!(
        manager
            .list_snapshot(None)
            .unwrap()
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "x", "a", "y", "b"]
    );

    let reloaded = TerminalManager::persistent(
        directory.path().to_path_buf(),
        TerminaldClient::new(directory.path().join("missing.sock")),
    )
    .unwrap();
    assert_eq!(
        reloaded
            .list_snapshot(Some("/workspace/one"))
            .unwrap()
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "a", "b"]
    );
    assert_eq!(
        reloaded
            .list_snapshot(Some("/workspace/two"))
            .unwrap()
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["x", "y"]
    );
}

#[tokio::test]
async fn delete_workspace_stops_runtimes_and_removes_only_matching_metadata() {
    let directory = TempDir::new().unwrap();
    let socket = directory.path().join("terminald").join("terminald.sock");
    let state_dir = directory.path().join("state");
    let target_root = directory.path().join("target");
    let other_root = directory.path().join("other");
    std::fs::create_dir_all(&target_root).unwrap();
    std::fs::create_dir_all(&other_root).unwrap();
    let target_root = target_root.to_string_lossy().into_owned();
    let other_root = other_root.to_string_lossy().into_owned();
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket.clone());
    let manager = TerminalManager::persistent(state_dir.clone(), client.clone()).unwrap();
    let terminal = manager
        .create(CreateTerminalRequest {
            name: Some("target terminal".to_owned()),
            cwd: Some("/tmp".to_owned()),
            workspace_root: Some(target_root.clone()),
            shell: Some("/bin/sh".to_owned()),
            agent_id: None,
            resume_session_id: None,
            rows: Some(24),
            cols: Some(80),
        })
        .await
        .unwrap();
    let agent = manager
        .create_agent(
            CreateTerminalRequest {
                name: Some("target agent".to_owned()),
                cwd: Some("/tmp".to_owned()),
                workspace_root: Some(target_root.clone()),
                shell: None,
                agent_id: Some("test-agent".to_owned()),
                resume_session_id: None,
                rows: Some(24),
                cols: Some(80),
            },
            AgentLaunch {
                agent_type: aow_agents::launch::AgentType::Codex,
                display_name: "Test Agent".to_owned(),
                executable: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), "sleep 30".to_owned()],
                env: Default::default(),
            },
        )
        .await
        .unwrap();
    let other = manager
        .create(CreateTerminalRequest {
            name: Some("other terminal".to_owned()),
            cwd: Some("/tmp".to_owned()),
            workspace_root: Some(other_root.clone()),
            shell: Some("/bin/sh".to_owned()),
            agent_id: None,
            resume_session_id: None,
            rows: Some(24),
            cols: Some(80),
        })
        .await
        .unwrap();

    assert_eq!(manager.workspace_tab_counts(&target_root).unwrap(), (1, 1));
    assert_eq!(
        manager.delete_workspace(&target_root).await.unwrap(),
        (1, 1)
    );
    assert!(
        manager
            .list_snapshot(Some(&target_root))
            .unwrap()
            .tabs
            .is_empty()
    );
    assert_eq!(
        manager.list_snapshot(Some(&other_root)).unwrap().tabs.len(),
        1
    );
    for pane in terminal.panes.iter().chain(&agent.panes) {
        assert!(client.get(&pane.id).await.unwrap().is_none());
    }
    assert!(client.get(&other.panes[0].id).await.unwrap().is_some());

    let reloaded = TerminalManager::persistent(state_dir, client).unwrap();
    assert!(
        reloaded
            .list_snapshot(Some(&target_root))
            .unwrap()
            .tabs
            .is_empty()
    );
    assert_eq!(
        reloaded
            .list_snapshot(Some(&other_root))
            .unwrap()
            .tabs
            .len(),
        1
    );
    stop_daemon(shutdown, daemon).await;
}

#[tokio::test]
async fn agent_pane_can_split_into_a_terminal_pane() {
    let directory = TempDir::new().unwrap();
    let socket = directory.path().join("terminald").join("terminald.sock");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket);
    let manager = TerminalManager::in_memory(client.clone());
    let tab = manager
        .create_agent(
            CreateTerminalRequest {
                name: Some("Test Agent".to_owned()),
                cwd: Some("/tmp".to_owned()),
                workspace_root: Some("/tmp".to_owned()),
                shell: None,
                agent_id: Some("test-agent".to_owned()),
                resume_session_id: None,
                rows: Some(24),
                cols: Some(80),
            },
            AgentLaunch {
                agent_type: aow_agents::launch::AgentType::Codex,
                display_name: "Test Agent".to_owned(),
                executable: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), "sleep 30".to_owned()],
                env: Default::default(),
            },
        )
        .await
        .unwrap();
    let agent_pane_id = tab.panes[0].id.clone();
    assert_eq!(tab.panes[0].agent_id.as_deref(), Some("codex"));

    let updated = manager
        .split(
            &tab.id,
            SplitTerminalRequest {
                target_pane_id: agent_pane_id.clone(),
                axis: TerminalSplitAxis::Row,
                ratio: None,
                cwd: None,
                shell: None,
                rows: None,
                cols: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.panes.len(), 2);
    let terminal = updated
        .panes
        .iter()
        .find(|pane| pane.id != agent_pane_id)
        .unwrap();
    assert_eq!(terminal.kind, TerminalPaneKind::Terminal);
    assert_eq!(terminal.agent_id, None);
    assert_eq!(terminal.shell, normalize_shell(None).unwrap());
    assert!(terminal.arguments.is_empty());
    assert!(matches!(
        updated.layout,
        TerminalLayout::Split {
            axis: TerminalSplitAxis::Row,
            ..
        }
    ));
    assert!(client.get(&agent_pane_id).await.unwrap().is_some());
    assert!(client.get(&terminal.id).await.unwrap().is_some());

    stop_daemon(shutdown, daemon).await;
}

#[test]
fn terminal_reorder_rejects_duplicate_or_stale_ids_without_mutating_state() {
    let manager = TerminalManager::in_memory(TerminaldClient::new(
        std::env::temp_dir().join("missing-terminald.sock"),
    ));
    manager.lock_state().unwrap().tabs =
        vec![tab_for("a", "/workspace"), tab_for("b", "/workspace")];

    assert!(matches!(
        manager.reorder(ReorderTerminalsRequest {
            workspace_root: "/workspace".to_owned(),
            tab_ids: vec!["a".to_owned(), "a".to_owned()],
        }),
        Err(TerminalError::Invalid(_))
    ));
    assert!(matches!(
        manager.reorder(ReorderTerminalsRequest {
            workspace_root: "/workspace".to_owned(),
            tab_ids: vec!["b".to_owned()],
        }),
        Err(TerminalError::Conflict(_))
    ));
    assert_eq!(
        manager
            .list_snapshot(Some("/workspace"))
            .unwrap()
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[tokio::test]
async fn terminal_reorder_route_returns_the_persisted_workspace_order() {
    let manager = TerminalManager::in_memory(TerminaldClient::new(
        std::env::temp_dir().join("missing-terminald.sock"),
    ));
    manager.lock_state().unwrap().tabs =
        vec![tab_for("a", "/workspace"), tab_for("b", "/workspace")];
    let response = clipboard_app(manager)
        .oneshot(
            Request::put("/api/terminals/order")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "workspace_root": "/workspace",
                        "tab_ids": ["b", "a"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = response_json(response).await;
    assert_eq!(response["tabs"][0]["id"], "b");
    assert_eq!(response["tabs"][1]["id"], "a");
}

#[tokio::test]
async fn terminal_name_intent_migrates_and_survives_explicit_rename() {
    let directory = TempDir::new().unwrap();
    let client = TerminaldClient::new(directory.path().join("missing.sock"));
    let manager =
        TerminalManager::persistent(directory.path().to_path_buf(), client.clone()).unwrap();
    let mut generated = tab_for("auto", "/workspace");
    generated.name = "Terminal 1".to_owned();
    let mut custom = tab_for("custom", "/workspace");
    custom.name = "My shell".to_owned();
    let mut agent = tab_for("agent", "/workspace");
    agent.name = "TraeCode CLI".to_owned();
    agent.panes[0].agent_id = Some("traecli".to_owned());
    agent.panes[0].name = "TraeCode CLI".to_owned();
    let mut cli = agent.clone();
    cli.id = "cli".to_owned();
    cli.panes[0].id = "cli-pane".to_owned();
    cli.layout = TerminalLayout::Pane {
        pane_id: "cli-pane".to_owned(),
    };
    cli.panes[0].agent_terminal = Some(aow_protocol::AgentTerminalState {
        phase: aow_protocol::AgentTerminalPhase::Ready,
        error: None,
        task_submitted: false,
    });
    manager.lock_state().unwrap().tabs = vec![generated, custom, agent, cli];
    manager
        .persist_locked(&manager.lock_state().unwrap())
        .unwrap();
    let manager =
        TerminalManager::persistent(directory.path().to_path_buf(), client.clone()).unwrap();
    assert_eq!(
        manager.get_snapshot("auto").unwrap().name_is_custom,
        Some(false)
    );
    assert_eq!(
        manager.get_snapshot("custom").unwrap().name_is_custom,
        Some(true)
    );
    let agent = manager.get_snapshot("agent").unwrap();
    assert_eq!(agent.name_is_custom, Some(false));
    let app = clipboard_app(manager.clone());
    for phase in [
        aow_protocol::AgentTerminalPhase::Starting,
        aow_protocol::AgentTerminalPhase::Ready,
        aow_protocol::AgentTerminalPhase::Failed,
    ] {
        manager
            .lock_state()
            .unwrap()
            .tabs
            .iter_mut()
            .find(|tab| tab.id == "cli")
            .unwrap()
            .panes[0]
            .agent_terminal
            .as_mut()
            .unwrap()
            .phase = phase;
        let before = manager.get_snapshot("cli").unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::patch("/api/terminals/cli")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"custom CLI name"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(manager.get_snapshot("cli").unwrap(), before);
    }
    manager
        .update(
            "agent",
            UpdateTerminalRequest {
                name: Some("User agent".to_owned()),
            },
        )
        .unwrap();
    assert_eq!(manager.get_snapshot("agent").unwrap().name, "User agent");
    manager
        .update(
            "auto",
            UpdateTerminalRequest {
                name: Some("Terminal 1".to_owned()),
            },
        )
        .unwrap();
    let manager = TerminalManager::persistent(directory.path().to_path_buf(), client).unwrap();
    let tab = manager.get_snapshot("auto").unwrap();
    assert_eq!(tab.name_is_custom, Some(true));
}

#[tokio::test]
async fn retired_pane_rename_route_does_not_change_metadata() {
    let directory = TempDir::new().unwrap();
    let manager = TerminalManager::persistent(
        directory.path().to_path_buf(),
        TerminaldClient::new(directory.path().join("missing.sock")),
    )
    .unwrap();
    manager
        .lock_state()
        .unwrap()
        .tabs
        .push(tab_for("tab-1", "/workspace"));
    manager
        .persist_locked(&manager.lock_state().unwrap())
        .unwrap();

    let response = clipboard_app(manager)
        .oneshot(
            Request::patch("/api/terminals/tab-1/panes/tab-1-pane")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"API server"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

    let reloaded = TerminalManager::persistent(
        directory.path().to_path_buf(),
        TerminaldClient::new(directory.path().join("missing.sock")),
    )
    .unwrap();
    let pane = &reloaded.get_snapshot("tab-1").unwrap().panes[0];
    assert_eq!(pane.name, "tmp");
    assert_eq!(pane.cwd, "/tmp");
}

#[test]
fn persistent_load_gives_legacy_panes_a_directory_name() {
    let directory = TempDir::new().unwrap();
    let state_path = directory.path().join(METADATA_FILE);
    let state = serde_json::json!({
        "version": METADATA_VERSION,
        "tabs": [{
            "id": "tab-1",
            "name": "Terminal",
            "workspace_root": "/workspace",
            "layout": { "type": "pane", "pane_id": "pane-1" },
            "panes": [{
                "id": "pane-1",
                "cwd": "/workspace/project",
                "shell": "/bin/bash",
                "status": "interrupted",
                "rows": 24,
                "cols": 80,
                "created_at": "2026-01-01T00:00:00.000Z",
                "updated_at": "2026-01-01T00:00:00.000Z"
            }],
            "revision": 1,
            "created_at": "2026-01-01T00:00:00.000Z",
            "updated_at": "2026-01-01T00:00:00.000Z"
        }]
    });
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let manager = TerminalManager::persistent(
        directory.path().to_path_buf(),
        TerminaldClient::new(directory.path().join("missing.sock")),
    )
    .unwrap();

    assert_eq!(
        manager.get_snapshot("tab-1").unwrap().panes[0].name,
        "project"
    );
}

#[test]
fn legacy_pane_custom_names_are_discarded_while_tab_names_survive() {
    let directory = TempDir::new().unwrap();
    let state_path = directory.path().join(METADATA_FILE);
    let mut tab = serde_json::to_value(tab_for("tab", "/workspace")).unwrap();
    tab["name"] = serde_json::json!("My tab");
    tab["name_is_custom"] = serde_json::json!(true);
    tab["panes"][0]["name"] = serde_json::json!("Obsolete pane label");
    for flag in [
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::Value::Null,
    ] {
        tab["panes"][0]["name_is_custom"] = flag;
        std::fs::write(
            &state_path,
            serde_json::to_vec(&serde_json::json!({
                "version": METADATA_VERSION, "tabs": [tab.clone()]
            }))
            .unwrap(),
        )
        .unwrap();
        let manager = TerminalManager::persistent(
            directory.path().to_path_buf(),
            TerminaldClient::new(directory.path().join("missing.sock")),
        )
        .unwrap();
        let loaded = manager.get_snapshot("tab").unwrap();
        assert_eq!(loaded.name, "My tab");
        assert_eq!(loaded.name_is_custom, Some(true));
        assert_eq!(loaded.panes[0].name, "tmp");
        assert_eq!(loaded.panes[0].cwd, "/tmp");
        assert!(
            serde_json::to_value(&loaded).unwrap()["panes"][0]
                .get("name_is_custom")
                .is_none()
        );
    }
}

#[test]
fn layout_revision_conflicts_are_rejected() {
    let manager = TerminalManager::in_memory(TerminaldClient::new("/tmp/missing.sock".into()));
    let tab = tab_with(
        TerminalLayout::Pane {
            pane_id: "a".into(),
        },
        vec![pane("a", TerminalPaneStatus::Interrupted)],
    );
    manager.lock_state().unwrap().tabs.push(tab.clone());
    assert!(matches!(
        manager.update_layout(
            &tab.id,
            UpdateLayoutRequest {
                layout: tab.layout.clone(),
                revision: Some(tab.revision - 1),
            }
        ),
        Err(TerminalError::Conflict(_))
    ));
}

#[test]
fn split_rollback_preserves_concurrent_tab_and_runtime_updates() {
    let manager = TerminalManager::in_memory(TerminaldClient::new("/tmp/missing.sock".into()));
    let previous = tab_with(
        TerminalLayout::Pane {
            pane_id: "a".into(),
        },
        vec![pane("a", TerminalPaneStatus::Running)],
    );
    let mut created = previous.clone();
    created.layout = TerminalLayout::Split {
        axis: TerminalSplitAxis::Row,
        ratio: 0.5,
        first: Box::new(TerminalLayout::Pane {
            pane_id: "a".into(),
        }),
        second: Box::new(TerminalLayout::Pane {
            pane_id: "failed".into(),
        }),
    };
    created
        .panes
        .push(pane("failed", TerminalPaneStatus::Running));
    created.revision += 1;

    let mut current = created.clone();
    current.name = "renamed concurrently".to_owned();
    current.revision += 1;
    current.panes[0].status = TerminalPaneStatus::Exited;
    current.panes[0].exit_code = Some(9);
    manager.lock_state().unwrap().tabs.push(current);
    manager
        .rollback_split(&created.id, "failed", &created, previous)
        .unwrap();

    let rolled_back = manager.get_snapshot(&created.id).unwrap();
    assert_eq!(rolled_back.name, "renamed concurrently");
    assert_eq!(rolled_back.panes.len(), 1);
    assert_eq!(rolled_back.panes[0].id, "a");
    assert_eq!(rolled_back.panes[0].status, TerminalPaneStatus::Exited);
    assert_eq!(rolled_back.panes[0].exit_code, Some(9));
    assert_eq!(
        rolled_back.layout,
        TerminalLayout::Pane {
            pane_id: "a".into()
        }
    );
}

#[test]
fn terminal_request_origin_must_match_request_host() {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "workspace.example:8080".parse().unwrap());
    assert!(validate_request_origin(&headers).is_ok());
    headers.insert(ORIGIN, "https://workspace.example:8080".parse().unwrap());
    assert!(validate_request_origin(&headers).is_ok());
    headers.insert(ORIGIN, "http://evil.example:8080".parse().unwrap());
    assert!(matches!(
        validate_request_origin(&headers),
        Err(TerminalError::ForbiddenOrigin(_))
    ));
}

#[test]
fn clipboard_magic_identifies_supported_formats() {
    assert_eq!(
        clipboard_image_type(&minimal_png()).unwrap().mime,
        "image/png"
    );
    assert_eq!(
        clipboard_image_type(&minimal_jpeg()).unwrap().mime,
        "image/jpeg"
    );
    assert_eq!(
        clipboard_image_type(&minimal_gif()).unwrap().mime,
        "image/gif"
    );
    assert_eq!(
        clipboard_image_type(&minimal_webp()).unwrap().mime,
        "image/webp"
    );
    assert_eq!(
        clipboard_image_type(&extended_webp()).unwrap().mime,
        "image/webp"
    );
    assert!(clipboard_image_type(b"\x89PNG\r\n\x1a\n").is_none());
    assert!(clipboard_image_type(b"<svg").is_none());
}

fn manager_with_pane() -> TerminalManager {
    let manager = TerminalManager::in_memory(TerminaldClient::new(
        std::env::temp_dir().join("missing-terminald.sock"),
    ));
    manager.lock_state().unwrap().tabs.push(tab_with(
        TerminalLayout::Pane {
            pane_id: "pane-1".to_owned(),
        },
        vec![pane("pane-1", TerminalPaneStatus::Running)],
    ));
    manager
}

fn clipboard_app(manager: TerminalManager) -> Router {
    routes().with_state(AppState {
        tasks: crate::tasks::TaskStore::new(
            None,
            None,
            crate::workspace_events::WorkspaceEvents::new(),
        )
        .unwrap(),
        base_path: crate::BasePath::default(),
        frontend_dist: PathBuf::new(),
        auth: crate::auth::AuthService::disabled(),
        session_shares: crate::session_shares::SessionShares::in_memory(),
        terminals: manager,
        aow: crate::aow::AowManager::in_memory(),
        automations: None,
        operations: crate::operations::OperationService::in_memory(),
        workspace_events: crate::workspace_events::WorkspaceEvents::new(),
        review_providers: crate::pull_requests::ProviderManager::default(),
    })
}

fn clipboard_request(pane_id: &str, body: Body) -> Request<Body> {
    Request::post(format!(
        "/api/terminals/tab-1/panes/{pane_id}/clipboard-images"
    ))
    .header(HOST, "workspace.example:8080")
    .header(ORIGIN, "https://workspace.example:8080")
    .body(body)
    .unwrap()
}

async fn response_json(response: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn clipboard_image_route_stores_private_png() {
    let manager = manager_with_pane();
    let directory = manager.inner.clipboard.directory.clone();
    let image = minimal_png();
    let response = clipboard_app(manager.clone())
        .oneshot(clipboard_request("pane-1", Body::from(image.clone())))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = response_json(response).await;
    assert_eq!(response["mime"], "image/png");
    assert_eq!(response["size"], image.len());
    assert!(response["expires_at"].as_str().unwrap().ends_with('Z'));

    let path = PathBuf::from(response["path"].as_str().unwrap());
    assert!(path.is_absolute());
    assert_eq!(path.parent(), Some(directory.as_path()));
    assert_eq!(
        path.extension().and_then(|value| value.to_str()),
        Some("png")
    );
    path.file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<aow_id::Snowflake>()
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), image);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[tokio::test]
async fn clipboard_image_route_rejects_unsupported_missing_pane_and_origin() {
    let manager = manager_with_pane();
    let app = clipboard_app(manager);

    let unsupported = app
        .clone()
        .oneshot(clipboard_request(
            "pane-1",
            Body::from("<svg xmlns='http://www.w3.org/2000/svg'/>"),
        ))
        .await
        .unwrap();
    assert_eq!(unsupported.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let missing = app
        .clone()
        .oneshot(clipboard_request(
            "missing",
            Body::from(&b"\x89PNG\r\n\x1a\n"[..]),
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let mut wrong_origin = clipboard_request("pane-1", Body::from(&b"\x89PNG\r\n\x1a\n"[..]));
    wrong_origin
        .headers_mut()
        .insert(ORIGIN, "https://evil.example:8080".parse().unwrap());
    let forbidden = app.oneshot(wrong_origin).await.unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn clipboard_image_route_preflights_content_length_limit() {
    let manager = manager_with_pane();
    let mut request = clipboard_request("pane-1", Body::empty());
    request.headers_mut().insert(
        CONTENT_LENGTH,
        (MAX_CLIPBOARD_IMAGE_BYTES + 1).to_string().parse().unwrap(),
    );
    let response = clipboard_app(manager).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn clipboard_image_stream_enforces_limit_and_removes_temp_file() {
    let manager = manager_with_pane();
    let result = manager
        .store_clipboard_image(
            "tab-1",
            "pane-1",
            Body::from(vec![0_u8; MAX_CLIPBOARD_IMAGE_BYTES as usize + 1]),
        )
        .await;
    assert!(matches!(result, Err(TerminalError::ClipboardImageTooLarge)));
    assert_eq!(
        std::fs::read_dir(&manager.inner.clipboard.directory)
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn clipboard_image_upload_rejects_a_pane_deleted_before_commit() {
    let manager = manager_with_pane();
    let image = minimal_png();
    let (sender, receiver) = mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let upload_manager = manager.clone();
    let upload = tokio::spawn(async move {
        upload_manager
            .store_clipboard_image("tab-1", "pane-1", Body::from_stream(stream))
            .await
    });
    sender
        .send(Ok(Bytes::copy_from_slice(&image[..20])))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if std::fs::read_dir(&manager.inner.clipboard.directory)
            .unwrap()
            .next()
            .is_some()
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::task::yield_now().await;
    }
    manager.lock_state().unwrap().tabs.clear();
    sender
        .send(Ok(Bytes::copy_from_slice(&image[20..])))
        .await
        .unwrap();
    drop(sender);
    let result = upload.await.unwrap();
    assert!(matches!(result, Err(TerminalError::TabNotFound(_))));
    assert_eq!(
        std::fs::read_dir(&manager.inner.clipboard.directory)
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn clipboard_image_upload_enforces_directory_quota() {
    let manager = manager_with_pane();
    let existing = manager.inner.clipboard.directory.join("existing.png");
    let file = std::fs::File::create(existing).unwrap();
    file.set_len(MAX_CLIPBOARD_DIRECTORY_BYTES).unwrap();

    let result = manager
        .store_clipboard_image("tab-1", "pane-1", Body::from(&b"\x89PNG\r\n\x1a\n"[..]))
        .await;
    assert!(matches!(result, Err(TerminalError::ClipboardQuotaExceeded)));
}

#[test]
fn clipboard_cleanup_removes_expired_files() {
    let manager = manager_with_pane();
    let directory = &manager.inner.clipboard.directory;
    let expired = directory.join("expired.png");
    let live = directory.join("live.png");
    std::fs::write(&expired, b"expired").unwrap();
    std::fs::write(&live, b"live").unwrap();
    let now = SystemTime::now();
    std::fs::File::options()
        .write(true)
        .open(&expired)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(now - CLIPBOARD_IMAGE_TTL - Duration::from_secs(1)),
        )
        .unwrap();

    let used = manager.inner.clipboard.clean_expired_at(now).unwrap();
    assert!(!expired.exists());
    assert!(live.exists());
    assert_eq!(used, 4);
}

#[test]
fn persistent_manager_uses_absolute_directory_and_cleans_on_creation() {
    let root = TempDir::new().unwrap();
    let state_dir = root.path().join("state");
    let directory = state_dir.join(CLIPBOARD_DIRECTORY);
    std::fs::create_dir_all(&directory).unwrap();
    let expired = directory.join("expired.png");
    std::fs::write(&expired, b"expired").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&expired)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(SystemTime::now() - CLIPBOARD_IMAGE_TTL - Duration::from_secs(1)),
        )
        .unwrap();

    let manager = TerminalManager::persistent(
        state_dir,
        TerminaldClient::new(root.path().join("missing.sock")),
    )
    .unwrap();
    assert!(manager.inner.clipboard.directory.is_absolute());
    assert_eq!(
        manager.inner.clipboard.directory,
        std::fs::canonicalize(directory).unwrap()
    );
    assert!(!expired.exists());
}

#[tokio::test]
async fn failed_create_and_split_roll_back_desired_state() {
    let directory = TempDir::new().unwrap();
    let manager =
        TerminalManager::in_memory(TerminaldClient::new(directory.path().join("missing.sock")));
    let create = manager
        .create(CreateTerminalRequest {
            name: Some("rollback".to_owned()),
            cwd: Some("/tmp".to_owned()),
            workspace_root: Some("/tmp".to_owned()),
            shell: Some("/bin/sh".to_owned()),
            agent_id: None,
            resume_session_id: None,
            rows: None,
            cols: None,
        })
        .await;
    assert!(matches!(create, Err(TerminalError::DaemonUnavailable(_))));
    assert!(manager.list_snapshot(None).unwrap().tabs.is_empty());

    let original = tab_with(
        TerminalLayout::Pane {
            pane_id: "existing".into(),
        },
        vec![pane("existing", TerminalPaneStatus::Running)],
    );
    manager.lock_state().unwrap().tabs.push(original.clone());
    let split = manager
        .split(
            &original.id,
            SplitTerminalRequest {
                target_pane_id: "existing".to_owned(),
                axis: TerminalSplitAxis::Row,
                ratio: None,
                cwd: None,
                shell: None,
                rows: None,
                cols: None,
            },
        )
        .await;
    assert!(matches!(split, Err(TerminalError::DaemonUnavailable(_))));
    assert_eq!(manager.get_snapshot(&original.id).unwrap(), original);
}

#[tokio::test]
async fn reconcile_cleans_orphans_rebuilds_after_daemon_restart_and_syncs_exit() {
    let directory = TempDir::new().unwrap();
    let socket = directory.path().join("terminald").join("terminald.sock");
    let state_dir = directory.path().join("state");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket.clone());
    let manager = TerminalManager::persistent(state_dir.clone(), client.clone()).unwrap();
    let tab = manager
        .create(CreateTerminalRequest {
            name: Some("reconcile".to_owned()),
            cwd: Some("/tmp".to_owned()),
            workspace_root: Some("/tmp".to_owned()),
            shell: Some("/bin/sh".to_owned()),
            agent_id: None,
            resume_session_id: None,
            rows: Some(24),
            cols: Some(80),
        })
        .await
        .unwrap();
    let pane_id = tab.panes[0].id.clone();
    client
        .create(
            "orphan",
            &TerminalRuntimeSpec {
                cwd: "/tmp".to_owned(),
                shell: "/bin/sh".to_owned(),
                arguments: Vec::new(),
                environment: Default::default(),
                rows: 24,
                cols: 80,
            },
        )
        .await
        .unwrap();
    manager.reconcile().await.unwrap();
    assert!(client.get("orphan").await.unwrap().is_none());

    let first_instance = client.health().await.unwrap().instance_id;
    stop_daemon(shutdown, daemon).await;
    assert_eq!(
        manager.get_snapshot(&tab.id).unwrap().panes[0].status,
        TerminalPaneStatus::Running
    );

    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let second_instance = client.health().await.unwrap().instance_id;
    assert_ne!(first_instance, second_instance);
    manager.reconcile().await.unwrap();
    assert_eq!(
        client.get(&pane_id).await.unwrap().unwrap().status,
        TerminalPaneStatus::Running
    );

    let mut attachment = client.attach(&pane_id).await.unwrap();
    attachment
        .send(tungstenite::Message::Binary(bytes::Bytes::from_static(
            b"exit 23\n",
        )))
        .await
        .unwrap();
    drop(attachment);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let runtime = client.get(&pane_id).await.unwrap().unwrap();
        if runtime.status == TerminalPaneStatus::Exited {
            assert_eq!(runtime.exit_code, Some(23));
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let updated = manager.get(&tab.id).await.unwrap();
    assert_eq!(updated.revision, tab.revision);
    assert_eq!(updated.panes[0].status, TerminalPaneStatus::Exited);
    assert_eq!(updated.panes[0].exit_code, Some(23));
    stop_daemon(shutdown, daemon).await;

    // Persisted panes remain desired state even after a natural exit. A
    // new daemon instance recreates every missing pane as a fresh shell.
    assert_eq!(
        manager.get_snapshot(&tab.id).unwrap().panes[0].status,
        TerminalPaneStatus::Exited
    );
    let (shutdown, daemon) = start_daemon(socket).await;
    manager.reconcile().await.unwrap();
    assert_eq!(
        manager.get_snapshot(&tab.id).unwrap().panes[0].status,
        TerminalPaneStatus::Running
    );
    stop_daemon(shutdown, daemon).await;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn terminal_agent_route_detects_start_exit_and_filters_workspaces() {
    let directory = TempDir::new().unwrap();
    let socket = directory.path().join("terminald/terminald.sock");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket);
    let manager = TerminalManager::in_memory(client.clone());
    let mut tabs = Vec::new();
    for name in ["a", "b"] {
        let workspace = directory.path().join(name);
        std::fs::create_dir(&workspace).unwrap();
        tabs.push(
            manager
                .create(CreateTerminalRequest {
                    name: Some("Shell".to_owned()),
                    cwd: Some(directory.path().to_string_lossy().into_owned()),
                    workspace_root: Some(workspace.to_string_lossy().into_owned()),
                    shell: Some("/bin/sh".to_owned()),
                    agent_id: None,
                    resume_session_id: None,
                    rows: None,
                    cols: None,
                })
                .await
                .unwrap(),
        );
    }
    let pane_id = &tabs[0].panes[0].id;
    let mut stream = client.attach(pane_id).await.unwrap();
    let mut other = client.attach(&tabs[1].panes[0].id).await.unwrap();
    other
        .send(tungstenite::Message::Binary(
            b"printf '\\033]2;Other workspace\\007'\n".to_vec().into(),
        ))
        .await
        .unwrap();
    drop(other);
    let app = clipboard_app(manager.clone());
    for (command, target, identity) in [
        ("codex", "codex", "codex"),
        ("claude", "claude", "claude"),
        ("traecli", "traecli", "traecli"),
        (".local/bin/traecli", "arbitrary-location/worker", "traecli"),
        ("custom/bin/traecli", "new-layout/renamed-worker", "traecli"),
    ] {
        let executable = directory.path().join(target);
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        let fixture = if cfg!(target_os = "macos") {
            std::env::current_exe().unwrap()
        } else {
            PathBuf::from("/bin/sh")
        };
        std::fs::copy(&fixture, &executable).unwrap();
        let entrypoint = directory.path().join(command);
        if entrypoint != executable {
            std::fs::create_dir_all(entrypoint.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&executable, &entrypoint).unwrap();
        }
        let command = if cfg!(target_os = "macos") {
            format!(
                "AOW_TEST_AGENT_ID={identity} '{}' --ignored --exact terminal::tests::native_agent_route_fixture --nocapture\n",
                entrypoint.display()
            )
        } else {
            format!(
                "'{}' -c 'printf \"\\033]0;Session {identity}\\007\"; while IFS= read -r title; do printf \"\\033]2;%s\\007\" \"$title\"; done'\n",
                entrypoint.display()
            )
        };
        stream
            .send(tungstenite::Message::Binary(command.into_bytes().into()))
            .await
            .unwrap();
        for expected in [Some(identity), None] {
            if expected.is_none() {
                stream
                    .send(tungstenite::Message::Binary(vec![3].into()))
                    .await
                    .unwrap();
            }
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let response = app
                    .clone()
                    .oneshot(
                        Request::get(format!(
                            "/api/terminals/agents?workspace_root={}/a",
                            directory.path().display()
                        ))
                        .body(Body::empty())
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(body["agents"].as_object().unwrap().len(), 1);
                assert!(body["titles"].get(&tabs[1].panes[0].id).is_none());
                assert!(body["processes"].get(&tabs[1].panes[0].id).is_none());
                if body["agents"][pane_id].as_str() == expected
                    && (expected.is_none()
                        || body["titles"][pane_id] == format!("Session {identity}"))
                {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "expected {expected:?}: {body}"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let all = manager.agents(None).await.unwrap();
            assert_eq!(all.agents.get(&tabs[1].panes[0].id), Some(&None));
            if expected.is_some() {
                let process = all.processes.get(pane_id).unwrap();
                assert!(process.pid > 0);
                assert!(!process.start_time.is_empty());
                assert_eq!(
                    process.cwd,
                    directory.path().canonicalize().unwrap().to_string_lossy()
                );
                if entrypoint != executable {
                    // An update changes the symlink target while the old
                    // process retains its public launch entrypoint.
                    let next_link = entrypoint.with_extension("next");
                    std::os::unix::fs::symlink(&fixture, &next_link).unwrap();
                    std::fs::rename(next_link, &entrypoint).unwrap();
                    tokio::time::sleep(Duration::from_millis(1100)).await;
                    let updated = manager.agents(None).await.unwrap();
                    assert_eq!(updated.agents[pane_id].as_deref(), Some(identity));
                    assert_eq!(updated.processes[pane_id].pid, process.pid);
                }
                stream
                    .send(tungstenite::Message::Binary(
                        b"Updated title\n".to_vec().into(),
                    ))
                    .await
                    .unwrap();
                // No browser attachment is needed to capture or retain titles.
                drop(stream);
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                loop {
                    if manager
                        .agents(None)
                        .await
                        .unwrap()
                        .titles
                        .get(pane_id)
                        .map(String::as_str)
                        == Some("Updated title")
                    {
                        break;
                    }
                    assert!(tokio::time::Instant::now() < deadline);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                stream = client.attach(pane_id).await.unwrap();
            }
        }
    }
    assert_eq!(manager.get_snapshot(&tabs[0].id).unwrap(), tabs[0]);
    drop(stream);
    stop_daemon(shutdown, daemon).await;
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "subprocess fixture invoked by the native Agent route test"]
fn native_agent_route_fixture() {
    use std::io::{BufRead, Write};
    let Ok(agent) = std::env::var("AOW_TEST_AGENT_ID") else {
        return;
    };
    let mut stdout = std::io::stdout().lock();
    write!(stdout, "\x1b]0;Session {agent}\x07").unwrap();
    stdout.flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        write!(stdout, "\x1b]2;{}\x07", line.unwrap()).unwrap();
        stdout.flush().unwrap();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn pane_sessions_use_live_cwd_and_config_and_cache_readable_claude_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let directory = TempDir::new().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let cwd = root.join("nested");
    let config = root.join("custom-claude");
    let bin = root.join("bin");
    let executable = root.join(".local/share/claude/versions/test");
    for path in [
        &cwd,
        &config.join("projects/demo"),
        &bin,
        executable.parent().unwrap(),
    ] {
        std::fs::create_dir_all(path).unwrap();
    }
    // macOS redacts the environment of SIP-protected /bin programs. Use
    // our own harmless fixture executable there, like an ordinary CLI.
    let (fixture, arguments) = if cfg!(target_os = "macos") {
        (
            std::env::current_exe().unwrap(),
            "--ignored --exact terminal::sessions::process::tests::native_environment_fixture",
        )
    } else {
        (
            PathBuf::from("/bin/sh"),
            "-c 'while read -r line; do :; done'",
        )
    };
    std::fs::copy(fixture, &executable).unwrap();
    std::fs::write(bin.join("claude"), "#!/bin/sh\n[ \"$1\" = agents ] && [ \"$2\" = --json ] || exit 9\ncat \"$CLAUDE_CONFIG_DIR/active.json\"\n").unwrap();
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let id = Uuid::new_v4().to_string();
    // The filename intentionally differs from the native session UUID.
    std::fs::write(config.join("projects/demo/transcript.jsonl"), format!("{}\n{}\n",
        serde_json::json!({"type":"user", "uuid":"prompt-1", "parentUuid":null, "sessionId":id, "cwd":cwd, "timestamp":"2026-09-17T00:00:00Z", "message":{"role":"user","content":"Read the current conversation"}}),
        serde_json::json!({"type":"custom-title", "customTitle":"Native session name"})
    )).unwrap();
    let socket = root.join("terminald/terminald.sock");
    let (shutdown, daemon) = start_daemon(socket.clone()).await;
    let client = TerminaldClient::new(socket);
    let manager = TerminalManager::in_memory(client.clone());
    let tab = manager
        .create(CreateTerminalRequest {
            name: None,
            cwd: Some(root.to_string_lossy().into_owned()),
            workspace_root: None,
            shell: Some("/bin/sh".into()),
            agent_id: None,
            resume_session_id: None,
            rows: None,
            cols: None,
        })
        .await
        .unwrap();
    let pane_id = &tab.panes[0].id;
    let mut stream = client.attach(pane_id).await.unwrap();
    stream.send(tungstenite::Message::Binary(format!(
        "cd '{}'; HOME='{}' CLAUDE_CONFIG_DIR='{}' PATH='{}:/usr/bin:/bin' AOW_NATIVE_ENV_FIXTURE=1 '{}' {arguments}\n",
        cwd.display(), root.display(), config.display(), bin.display(), executable.display()
    ).into_bytes().into())).await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let process = loop {
        if let Some(process) = manager.agents(None).await.unwrap().processes.get(pane_id) {
            break process.clone();
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    std::fs::write(
        config.join("active.json"),
        serde_json::json!([
            {"pid":process.pid, "id":"short-job", "sessionId":id, "cwd":cwd}
        ])
        .to_string(),
    )
    .unwrap();
    let app = crate::build_router(AppState {
        tasks: crate::tasks::TaskStore::new(
            None,
            None,
            crate::workspace_events::WorkspaceEvents::new(),
        )
        .unwrap(),
        base_path: crate::BasePath::default(),
        frontend_dist: PathBuf::new(),
        auth: crate::auth::AuthService::disabled(),
        session_shares: crate::session_shares::SessionShares::in_memory(),
        terminals: manager.clone(),
        aow: crate::aow::AowManager::in_memory(),
        automations: None,
        operations: crate::operations::OperationService::in_memory(),
        workspace_events: crate::workspace_events::WorkspaceEvents::new(),
        review_providers: crate::pull_requests::ProviderManager::default(),
    });
    let response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/terminals/{}/panes/{pane_id}/agent-sessions",
                tab.id
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cwd"], cwd.to_string_lossy().as_ref());
    assert_eq!(body["live_session_id"], id);
    assert_eq!(body["sessions"][0]["title"], "Native session name");
    assert_eq!(body["sessions"][0]["session_id"], id);
    let response = app
        .oneshot(
            Request::get(format!(
                "/api/aow/agent-sessions/{id}/snapshot?agent=claude&worktree_path={}",
                cwd.display()
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response_json(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["turns"][0]["user"]["text"],
        "Read the current conversation"
    );
    // Continue through the real notification loop using this same native
    // PID and custom config, not a manually registered transcript reader.
    let mut stops = manager.inner.task_stops.subscribe();
    manager.start_agent_notifications(crate::aow::AowManager::in_memory());
    let event = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            use std::io::Write;
            let mut transcript = std::fs::OpenOptions::new()
                .append(true)
                .open(config.join("projects/demo/transcript.jsonl"))
                .unwrap();
            writeln!(
                transcript,
                "{}",
                serde_json::json!({
                    "type":"system", "subtype":"turn_duration", "sessionId":id
                })
            )
            .unwrap();
            match tokio::time::timeout(Duration::from_millis(200), stops.recv()).await {
                Ok(event) => break event.unwrap(),
                Err(_) => continue, // first registration starts at EOF
            }
        }
    })
    .await
    .expect("native process completion was not delivered");
    assert_eq!(event.agent, "claude");
    assert_eq!(event.session_id, id);
    assert_eq!(event.instance_ids, [pane_id.clone()]);
    assert_eq!(manager.get_snapshot(&tab.id).unwrap(), tab);
    drop(stream);
    stop_daemon(shutdown, daemon).await;
}

pub(super) async fn start_daemon(
    socket: PathBuf,
) -> (
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), aow_terminald::TerminaldError>>,
) {
    let (shutdown, stopped) = oneshot::channel();
    let daemon_socket = socket.clone();
    let daemon = tokio::spawn(async move {
        aow_terminald::run_with_shutdown(daemon_socket, async move {
            let _ = stopped.await;
        })
        .await
    });
    let client = TerminaldClient::new(socket);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if client.health().await.is_ok() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "terminald did not become ready"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (shutdown, daemon)
}

pub(super) async fn stop_daemon(
    shutdown: oneshot::Sender<()>,
    daemon: tokio::task::JoinHandle<Result<(), aow_terminald::TerminaldError>>,
) {
    let _ = shutdown.send(());
    tokio::time::timeout(std::time::Duration::from_secs(10), daemon)
        .await
        .expect("terminald shutdown timed out")
        .expect("terminald task panicked")
        .expect("terminald shutdown failed");
}
