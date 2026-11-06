use super::*;
use serde_json::json;

#[tokio::test]
async fn main_conversations_stay_bound_and_protected_until_archive_or_task_deletion() {
    let (_directory, state, project, _) = tests::workspace_fixture().await;
    let app = routes()
        .merge(crate::acp::routes())
        .with_state(state.clone());
    for orchestrator in [false, true] {
        let id = if orchestrator { "orchestrator" } else { "task" };
        let (code, task) = tests::call(
            &app,
            "POST",
            "/api/tasks",
            json!({
                "request_id":id,"project_id":project.id,"prompt":"Inspect the project",
                "agent_id":"test","workspace_mode":"existing","workspace":project.registered_path,
                "is_orchestrator":orchestrator
            }),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{task}");
        let main = task["initial_thread_id"].as_str().unwrap();
        tests::task_idle(&state, main).await;
        let auxiliary = state
            .acp
            .prepare_session("test", project.registered_path.clone().into())
            .await
            .unwrap();
        state
            .tasks
            .attach_task_resource(Some(id), "acp", &auxiliary.thread_id)
            .unwrap();
        let path = format!("/api/acp/sessions/{main}");
        // Stopping the process must retain the record and its identity.
        state.acp.close_session(main).await.unwrap();
        assert!(state.acp.session_info(main).await.is_ok());
        for status in if orchestrator {
            vec!["todo"]
        } else {
            vec!["done", "cancelled", "blocked", "in_progress"]
        } {
            if !orchestrator {
                assert_eq!(
                    tests::call(
                        &app,
                        "POST",
                        &format!("/api/tasks/{id}/status"),
                        json!({"status":status})
                    )
                    .await
                    .0,
                    StatusCode::OK
                );
            }
            assert_eq!(
                tests::call(&app, "DELETE", &path, json!({})).await.0,
                StatusCode::BAD_REQUEST
            );
            let stored = state
                .tasks
                .list()
                .unwrap()
                .into_iter()
                .find(|task| task.id == id)
                .unwrap();
            assert_eq!(stored.initial_thread_id.as_deref(), Some(main));
            assert_eq!(stored.resources.len(), 2);
        }
        assert_eq!(
            tests::call(
                &app,
                "DELETE",
                &format!("/api/acp/sessions/{}", auxiliary.thread_id),
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert!(state.acp.session_info(main).await.is_ok());
        let task_path = format!("/api/tasks/{id}");
        assert_eq!(
            tests::call(&app, "PATCH", &task_path, json!({"archived":true}))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert!(
            state.acp.session_info(main).await.is_ok(),
            "archive retains history"
        );
        assert_eq!(
            tests::call(&app, "PATCH", &task_path, json!({"archived":false}))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            tests::call(&app, "DELETE", &path, json!({})).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            tests::call(&app, "PATCH", &task_path, json!({"archived":true}))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            tests::call(&app, "DELETE", &path, json!({})).await.0,
            StatusCode::OK
        );
        assert_eq!(
            tests::call(&app, "PATCH", &task_path, json!({"archived":false}))
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "never replace a cleaned main conversation"
        );
        assert_eq!(
            tests::call(&app, "DELETE", &task_path, json!({})).await.0,
            StatusCode::NO_CONTENT
        );

        // Deleting the space releases protection without first deleting history.
        let (code, replacement) = if orchestrator {
            tests::call(
                &app,
                "POST",
                &format!("/api/projects/{}/orchestrator", project.id),
                json!({"agent_id":"test"}),
            )
            .await
        } else {
            tests::call(&app, "POST", "/api/tasks", json!({
            "request_id":format!("replacement-{id}"),"project_id":project.id,"prompt":"Inspect again","agent_id":"test",
            "workspace_mode":"existing","workspace":project.registered_path,"is_orchestrator":orchestrator
        })).await
        };
        assert_eq!(code, StatusCode::OK, "{replacement}");
        let next = replacement["initial_thread_id"].as_str().unwrap();
        assert_ne!(main, next);
        tests::task_idle(&state, next).await;
        assert_eq!(
            tests::call(
                &app,
                "DELETE",
                &format!("/api/tasks/{}", replacement["id"].as_str().unwrap()),
                json!({})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            tests::call(
                &app,
                "DELETE",
                &format!("/api/acp/sessions/{next}"),
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
}

#[tokio::test]
async fn workspace_cleanup_cannot_delete_an_active_tasks_main_history() {
    let (_directory, state, project, _) = tests::workspace_fixture().await;
    let app = routes()
        .merge(crate::projects::worktrees::routes())
        .with_state(state.clone());
    let (code, task) = tests::call(
        &app,
        "POST",
        "/api/tasks",
        json!({
            "request_id":"isolated","project_id":project.id,"prompt":"Inspect", "agent_id":"test",
            "workspace_mode":"new_worktree"
        }),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{task}");
    let thread = task["initial_thread_id"].as_str().unwrap();
    tests::task_idle(&state, thread).await;
    let path = task["workspace"].as_str().unwrap();
    let (code, _) = tests::call(
        &app,
        "POST",
        &format!("/api/projects/{}/worktrees/removals", project.id),
        json!({"items":[{"path":path,"force":true}]}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(Path::new(path).is_dir());
    assert!(state.acp.session_info(thread).await.is_ok());
    assert!(
        state
            .tasks
            .ensure_workspace_deletable(&state.acp, path)
            .await
            .is_err()
    );
    assert_eq!(
        tests::call(
            &app,
            "PATCH",
            "/api/tasks/isolated",
            json!({"archived":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        state
            .tasks
            .ensure_workspace_deletable(&state.acp, path)
            .await
            .is_ok()
    );
    state.acp.close_session(thread).await.unwrap();
}
