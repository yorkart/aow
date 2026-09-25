use super::*;
use std::time::Duration;

#[derive(Serialize)]
pub(super) struct ProjectAvatar {
    avatar_url: Option<String>,
}

pub(super) async fn get_avatar(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<ProjectAvatar>, Response> {
    let stored = state
        .aow
        .lock()
        .map_err(aow_response)?
        .projects
        .iter()
        .find(|project| project.id == id)
        .cloned()
        .ok_or_else(|| aow_response(AowError::ProjectNotFound(id.clone())))?;
    if stored.builtin {
        return Ok(Json(ProjectAvatar { avatar_url: None }));
    }
    let lookup = async {
        let paths = state
            .aow
            .execution_path()
            .await
            .map_err(|error| error.to_string())?;
        state
            .review_providers
            .repository_avatar(&stored.registered_path, &paths)
            .await
            .map_err(|error| error.to_string())
    };
    let avatar_url = match tokio::time::timeout(Duration::from_secs(12), lookup).await {
        Ok(Ok(url)) => url,
        result => {
            tracing::debug!(project_id = %id, error = ?result, "Project avatar lookup unavailable");
            return Ok(Json(ProjectAvatar {
                avatar_url: stored.avatar_url,
            }));
        }
    };
    let mut registry = state.aow.lock().map_err(aow_response)?;
    let index = registry
        .projects
        .iter()
        .position(|project| project.id == id)
        .ok_or_else(|| aow_response(AowError::ProjectNotFound(id)))?;
    // A registration can change the directory while the provider is running.
    if registry.projects[index].registered_path != stored.registered_path {
        return Ok(Json(ProjectAvatar {
            avatar_url: registry.projects[index].avatar_url.clone(),
        }));
    }
    if registry.projects[index].avatar_url != avatar_url {
        let previous = registry.projects[index].avatar_url.clone();
        registry.projects[index].avatar_url = avatar_url.clone();
        if let Err(error) = state.aow.persist_projects(&registry.projects) {
            registry.projects[index].avatar_url = previous;
            return Err(aow_response(error));
        }
    }
    Ok(Json(ProjectAvatar { avatar_url }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    async fn request(app: &Router, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn provider(app: &Router, result: &str) {
        let (_, current) = request(app, "GET", "/api/aow/review-providers", Value::Null).await;
        let script = format!(
            "import json,sys\nr=json.load(sys.stdin)\nif r['operation']=='describe':\n result={{'operations':['list','detail','diff','repository_info']}}\nelse:\n assert r['operation']=='repository_info'\n result={result}\nprint(json.dumps({{'version':2,'result':result}}))"
        );
        let (status, _) = request(app, "PUT", "/api/aow/review-providers", json!({
            "revision": current["revision"],
            "providers": [{"id":"test","name":"Test","enabled":true,"hosts":["git.example.com"],"script":script}]
        })).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn project_avatar_endpoint_persists_urls_preserves_saved_urls_on_failure_and_clears_null()
    {
        let directory = tempfile::tempdir().unwrap();
        let repository = directory.path().join("repo");
        std::fs::create_dir(&repository).unwrap();
        for args in [
            vec!["init", "--quiet"],
            vec![
                "remote",
                "add",
                "origin",
                "git@git.example.com:team/repo.git",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&repository)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let state_dir = directory.path().join("state");
        let manager =
            AowManager::persistent_with_notes_base(&state_dir, directory.path().join("notes"))
                .unwrap();
        manager.lock_settings().unwrap().execution_path =
            Some(vec!["/usr/bin".into(), "/bin".into()]);
        let mut state = AppState::new(PathBuf::new());
        state.aow = manager.clone();
        let app = crate::build_router(state);
        provider(
            &app,
            "{'avatar_url':'https://avatars.example.com/team.png'}",
        )
        .await;
        let (status, project) = request(
            &app,
            "POST",
            "/api/aow/projects",
            json!({"path":repository}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(
            project["avatar_url"].is_null(),
            "registration does not wait for platform metadata"
        );
        let id = project["id"].as_str().unwrap();
        let endpoint = format!("/api/aow/projects/{id}/avatar");
        let (status, avatar) = request(&app, "GET", &endpoint, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(avatar["avatar_url"], "https://avatars.example.com/team.png");
        let (_, projects) = request(&app, "GET", "/api/aow/projects", Value::Null).await;
        assert_eq!(projects[0]["avatar_url"], avatar["avatar_url"]);
        let restored =
            AowManager::persistent_with_notes_base(&state_dir, directory.path().join("notes"))
                .unwrap();
        assert_eq!(
            restored.project(id).await.unwrap().avatar_url.as_deref(),
            Some("https://avatars.example.com/team.png")
        );

        provider(&app, "1/0").await;
        assert_eq!(request(&app, "GET", &endpoint, Value::Null).await.1, avatar);
        provider(&app, "{'avatar_url':None}").await;
        assert!(request(&app, "GET", &endpoint, Value::Null).await.1["avatar_url"].is_null());
        assert!(manager.project(id).await.unwrap().avatar_url.is_none());

        manager.lock().unwrap().projects[0].builtin = true;
        provider(
            &app,
            "{'avatar_url':'https://avatars.example.com/should-not-load.png'}",
        )
        .await;
        assert!(request(&app, "GET", &endpoint, Value::Null).await.1["avatar_url"].is_null());
        assert_eq!(
            request(&app, "GET", "/api/aow/projects/missing/avatar", Value::Null)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
}
