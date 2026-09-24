use super::*;
use aow_config::{ConfigRepository, ConfigSelection, RepositoryVersions};

#[derive(Debug, Serialize)]
struct ConfigurationSettings {
    selection: ConfigSelection,
    active_selection: ConfigSelection,
    restart_required: bool,
    changed: bool,
}

#[derive(Deserialize)]
struct RepositoryQuery {
    path: String,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/aow/settings/configuration",
            get(get_configuration).put(save_configuration),
        )
        .route(
            "/api/aow/settings/configuration/versions",
            get(list_versions),
        )
}

impl AowManager {
    fn configuration_settings(
        &self,
        selection: ConfigSelection,
        changed: bool,
    ) -> Result<ConfigurationSettings, AowError> {
        let active_selection = self
            .inner
            .config
            .as_ref()
            .ok_or_else(|| AowError::Invalid("当前服务未配置持久化数据目录".into()))?
            .selection();
        Ok(ConfigurationSettings {
            restart_required: selection != active_selection,
            selection,
            active_selection,
            changed,
        })
    }

    fn lock_configuration_file(&self) -> Result<MutexGuard<'_, aow_config::ConfigFile>, AowError> {
        self.inner
            .configuration_file
            .as_ref()
            .ok_or_else(|| AowError::Invalid("当前服务未配置持久化数据目录".into()))?
            .lock()
            .map_err(|_| AowError::Poisoned)
    }
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, AowError> + Send + 'static,
) -> Result<Json<T>, Response> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| aow_response(AowError::Configuration(error.into())))?
        .map(Json)
        .map_err(aow_response)
}

async fn get_configuration(
    State(state): State<AppState>,
) -> Result<Json<ConfigurationSettings>, Response> {
    let selection = state
        .aow
        .lock_configuration_file()
        .map_err(aow_response)?
        .selection()
        .clone();
    state
        .aow
        .configuration_settings(selection, false)
        .map(Json)
        .map_err(aow_response)
}

async fn list_versions(
    Query(query): Query<RepositoryQuery>,
) -> Result<Json<RepositoryVersions>, Response> {
    blocking(move || {
        let path = query.path.trim();
        let path = if path == "~" {
            crate::PROCESS_HOME.clone()
        } else if let Some(relative) = path.strip_prefix("~/") {
            crate::PROCESS_HOME.join(relative)
        } else {
            PathBuf::from(path)
        };
        aow_config::inspect_repository(&path)
            .map_err(|error| AowError::Invalid(format!("{error:#}")))
    })
    .await
}

async fn save_configuration(
    State(state): State<AppState>,
    Json(selection): Json<ConfigSelection>,
) -> Result<Json<ConfigurationSettings>, Response> {
    blocking(move || {
        // Validate requests separately so invalid repositories/versions return 400.
        ConfigRepository::from_selection(&selection)
            .map_err(|error| AowError::Invalid(format!("{error:#}")))?;
        let mut file = state.aow.lock_configuration_file()?;
        let changed = file.save(&selection).map_err(AowError::Configuration)?;
        state
            .aow
            .configuration_settings(file.selection().clone(), changed)
    })
    .await
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

    async fn call(router: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn configuration_routes_validate_and_persist_without_switching_running_service() {
        let temp = tempfile::tempdir().unwrap();
        let state_dir = temp.path().join("state");
        let target = ConfigRepository::initialize(&temp.path().join("target"))
            .unwrap()
            .selection();
        let obsolete = target
            .config_repo
            .join(&target.config_id)
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir(&state_dir).unwrap();
        std::fs::write(state_dir.join("__current__"), &obsolete).unwrap();
        let mut state = AppState::new(temp.path().join("frontend"));
        state.aow =
            AowManager::persistent_with_notes_base(&state_dir, temp.path().join("notes")).unwrap();
        let active = state.aow.inner.config.as_ref().unwrap().selection();
        assert_ne!(active, target);
        let second_id = "550e8400-e29b-41d4-a716-446655440000";
        std::fs::create_dir(target.config_repo.join(second_id)).unwrap();
        let router = routes().with_state(state.clone());
        let endpoint = "/api/aow/settings/configuration";
        // External edits must not be reread by Settings GET or PUT. Even invalid
        // TOML cannot change or break a process that already loaded its config.
        std::fs::write(state_dir.join("config.toml"), "invalid = [").unwrap();
        let (code, settings) = call(&router, "GET", endpoint, Value::Null).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(settings["selection"], json!(active));
        assert_eq!(settings["restart_required"], false);

        let query = format!(
            "{endpoint}/versions?path={}",
            target.config_repo.join(&target.config_id).display()
        );
        let (code, choices) = call(&router, "GET", &query, Value::Null).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(choices["config_repo"], json!(target.config_repo));
        assert_eq!(choices["selected_id"], target.config_id);
        assert_eq!(choices["config_ids"].as_array().unwrap().len(), 2);
        let invalid_query = format!("{endpoint}/versions?path={}", state_dir.display());
        let (code, error) = call(&router, "GET", &invalid_query, Value::Null).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(error["message"].as_str().unwrap().contains("Git 仓库"));

        let before = std::fs::read(state_dir.join("config.toml")).unwrap();
        let (code, _) = call(
            &router,
            "PUT",
            endpoint,
            json!({ "config_repo": target.config_repo, "config_id": "../bad" }),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(
            std::fs::read(state_dir.join("config.toml")).unwrap(),
            before
        );

        let (code, updated) = call(&router, "PUT", endpoint, json!(target)).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(updated["changed"], true);
        assert_eq!(updated["restart_required"], true);
        assert_eq!(updated["active_selection"], json!(active));
        assert_eq!(updated["selection"], json!(target));
        assert_eq!(
            std::fs::read_to_string(state_dir.join("__current__")).unwrap(),
            obsolete
        );
        let (_, unchanged) = call(&router, "PUT", endpoint, json!(target)).await;
        assert_eq!(unchanged["changed"], false);
        assert_eq!(unchanged["restart_required"], true);
        let saved_document = std::fs::read(state_dir.join("config.toml")).unwrap();
        std::fs::remove_file(state_dir.join("config.toml")).unwrap();
        let (_, reread) = call(&router, "GET", endpoint, Value::Null).await;
        assert_eq!(reread, unchanged);
        // Other settings still write to the running version until a restart.
        state
            .aow
            .update_settings(UpdateSettingsRequest {
                editor: Some(EditorSettings { word_wrap: true }),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            active
                .config_repo
                .join(&active.config_id)
                .join(SETTINGS_FILE)
                .is_file()
        );
        assert!(
            !target
                .config_repo
                .join(&target.config_id)
                .join(SETTINGS_FILE)
                .exists()
        );
        std::fs::write(state_dir.join("config.toml"), saved_document).unwrap();
        let restarted =
            AowManager::persistent_with_notes_base(&state_dir, temp.path().join("notes")).unwrap();
        assert!(
            !restarted
                .configuration_settings(target.clone(), false)
                .unwrap()
                .restart_required
        );
        assert!(!restarted.settings().unwrap().editor.word_wrap);

        let (_, reverted) = call(&router, "PUT", endpoint, json!(active)).await;
        assert_eq!(reverted["changed"], true);
        assert_eq!(reverted["restart_required"], false);
        std::fs::remove_file(state_dir.join("config.toml")).unwrap();
        std::fs::create_dir(state_dir.join("config.toml")).unwrap();
        let (code, _) = call(&router, "PUT", endpoint, json!(target)).await;
        assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(state.aow.inner.config.as_ref().unwrap().selection(), active);
    }
}
