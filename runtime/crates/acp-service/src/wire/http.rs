use super::*;
use axum::{
    Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    routing::{get, put},
};
use instance::{Attachment, MAX_MESSAGE_BYTES, valid_message};
use std::time::Duration;

pub(super) fn router(state: WireRuntime) -> Router {
    Router::new()
        .route("/acp", get(connect))
        .route(
            "/v1/workspaces/removing",
            axum::routing::put(workspace_removing),
        )
        .route(
            "/v1/workspaces/recover",
            axum::routing::post(recover_workspaces),
        )
        .route("/v1/acp/settings", get(settings).put(save_settings))
        .route("/v1/acp/registry", get(registry))
        .route("/v1/acp/agents/{id}/install", axum::routing::post(install))
        .route("/v1/acp/agents/{id}/installation", get(installation))
        .route("/v1/environment", get(environment).put(save_environment))
        .route(
            "/v1/directories/resolve",
            axum::routing::post(resolve_directory),
        )
        .route("/v1/files/read", axum::routing::post(read_file))
        .route("/v1/files/write", axum::routing::post(write_file))
        .route("/v1/acp/agents", get(agents))
        .route("/v1/acp/instances", get(list))
        .route(
            "/v1/acp/instances/{id}",
            put(create).get(describe).delete(delete),
        )
        .route("/v1/acp/instances/{id}/acp", get(attach))
        .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state)
}

async fn agents(State(state): State<WireRuntime>) -> Result<Json<serde_json::Value>, ApiError> {
    let agents = state.service.agents().await.map_err(ApiError::invalid)?;
    Ok(Json(serde_json::json!({"agents": agents})))
}
async fn list(State(state): State<WireRuntime>) -> Json<serde_json::Value> {
    let instances: Vec<_> = state
        .instances
        .lock()
        .await
        .values()
        .map(|i| i.info())
        .collect();
    Json(serde_json::json!({"instances": instances}))
}
async fn describe(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
) -> Result<Json<InstanceInfo>, ApiError> {
    Ok(Json(state.get(&id).await?.info()))
}
async fn create(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
    Json(spec): Json<InstanceSpec>,
) -> Result<Response, ApiError> {
    // Creation owns its lifecycle even if an HTTP client disconnects mid-installation.
    tokio::spawn(async move {
        let (instance, created) = state.create(id, spec).await?;
        Ok((
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            Json(instance.info()),
        )
            .into_response())
    })
    .await
    .map_err(ApiError::invalid)?
}
async fn delete(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    tokio::spawn(async move {
        state.delete(&id).await;
    })
    .await
    .map_err(ApiError::invalid)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn attach(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    upgrade(ws, state.get(&id).await?.attach()?, None)
}
async fn connect(
    State(state): State<WireRuntime>,
    Query(spec): Query<InstanceSpec>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let id = uuid::Uuid::new_v4().to_string();
    let (instance, _) = state.create(id.clone(), spec).await?;
    upgrade(ws, instance.attach()?, Some((state, id)))
}
fn upgrade(
    ws: WebSocketUpgrade,
    attachment: Attachment,
    cleanup: Option<(WireRuntime, String)>,
) -> Result<Response, ApiError> {
    let id = attachment.instance.id.clone();
    let failed_cleanup = cleanup.clone();
    let mut response = ws
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_failed_upgrade(move |_| {
            if let Some((state, id)) = failed_cleanup {
                tokio::spawn(async move {
                    state.delete(&id).await;
                });
            }
        })
        .on_upgrade(move |socket| async move {
            socket_loop(socket, attachment).await;
            if let Some((state, id)) = cleanup {
                state.delete(&id).await;
            }
        });
    response
        .headers_mut()
        .insert("acp-connection-id", id.parse().map_err(ApiError::invalid)?);
    Ok(response)
}
async fn socket_loop(mut socket: WebSocket, mut attachment: Attachment) {
    let mut initialized = false;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await;
    let mut last_seen = tokio::time::Instant::now();
    let result: Result<()> = async {
        loop {
            tokio::select! {
                output = attachment.output.recv() => {
                    let Some(output) = output else { break; };
                    send(&mut socket, Message::Text(output.into())).await?;
                },
                input = socket.recv() => {
                    let Some(input) = input else { break; };
                    last_seen = tokio::time::Instant::now();
                    match input? {
                        Message::Text(text) => {
                            let value = match valid_message(&text) {
                                Ok(value) => value,
                                Err(code) => {
                                    send(&mut socket, Message::Text(serde_json::json!({"jsonrpc":"2.0","id":null,"error":{"code":code,"message":"Invalid JSON-RPC message"}}).to_string().into())).await?;
                                    continue;
                                }
                            };
                            if !initialized {
                                ensure!(value["method"] == "initialize" && value.get("id").is_some(), "First ACP message must be initialize");
                                initialized = true;
                            }
                            // Stdio is newline delimited, so normalize JSON whitespace only.
                            attachment.instance.send(serde_json::to_string(&value)?).await?;
                        },
                        Message::Close(_) => break,
                        Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => {},
                    }
                },
                _ = heartbeat.tick() => {
                    ensure!(last_seen.elapsed() < Duration::from_secs(45), "ACP peer stopped responding");
                    send(&mut socket, Message::Ping(vec![].into())).await?;
                }
            }
        }
        Ok(())
    }.await;
    if let Err(error) = result {
        tracing::debug!(%error, "ACP WebSocket closed");
    }
    attachment.instance.stop().await;
    let _ = send(&mut socket, Message::Close(None)).await;
}
async fn send(socket: &mut WebSocket, message: Message) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await??;
    Ok(())
}

async fn settings(State(state): State<WireRuntime>) -> Result<Json<serde_json::Value>, ApiError> {
    use aow_acp_core::AcpHost;
    let content = state.host.settings().await.map_err(ApiError::invalid)?;
    Ok(Json(
        serde_json::json!({"path":state.host.root.join("aow-acp.json"),"revision":format!("{:x}",md5::compute(&content)),"content":content}),
    ))
}
use aow_runtime_contract::acp::AcpSettingsUpdate as SaveSettings;
async fn save_settings(
    State(state): State<WireRuntime>,
    Json(request): Json<SaveSettings>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use aow_acp_core::AcpHost;
    aow_acp_core::validate_settings(&request.content).map_err(ApiError::invalid)?;
    let guard = state.settings_lock.lock().await;
    let previous = state.host.settings().await.map_err(ApiError::invalid)?;
    if format!("{:x}", md5::compute(&previous)) != request.revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "ACP settings changed; reload before saving".into(),
        ));
    }
    state
        .host
        .write_text_file(
            &state.host.root,
            &state.host.root.join("aow-acp.json"),
            &request.content,
        )
        .await
        .map_err(ApiError::invalid)?;
    drop(guard);
    settings(State(state)).await
}
#[derive(Deserialize)]
struct RegistryQuery {
    #[serde(default)]
    refresh: bool,
}
async fn registry(
    State(state): State<WireRuntime>,
    Query(query): Query<RegistryQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(
        serde_json::json!({"agents":state.service.registry(query.refresh).await.map_err(ApiError::invalid)?}),
    ))
}
async fn install(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    tokio::spawn(async move {
        let mut stop=state.stopping.clone();
        if *stop.borrow(){anyhow::bail!("Runtime is stopping");}
        tokio::select! {result=state.service.install_agent(&id)=>result,_=stop.changed()=>Err(anyhow::anyhow!("Runtime is stopping"))}
    })
        .await
        .map_err(ApiError::invalid)?
        .map_err(ApiError::invalid)?;
    Ok(Json(serde_json::json!({})))
}
async fn installation(
    State(state): State<WireRuntime>,
    Path(id): Path<String>,
) -> Json<Option<aow_acp_core::ConnectionStatus>> {
    Json(state.service.installation_status(&id))
}
async fn environment(
    State(state): State<WireRuntime>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use aow_acp_core::AcpHost;
    let path = state
        .host
        .execution_path()
        .await
        .map_err(ApiError::invalid)?;
    let path = path.map(|path| std::env::split_paths(&path).collect::<Vec<_>>());
    Ok(Json(
        serde_json::json!({"execution_path":path,"state_dir":state.host.root}),
    ))
}
#[derive(Deserialize)]
struct FileRequest {
    cwd: PathBuf,
    path: PathBuf,
    content: Option<String>,
}
async fn read_file(
    State(state): State<WireRuntime>,
    Json(request): Json<FileRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use aow_acp_core::AcpHost;
    if !request.cwd.is_absolute() || !request.path.is_absolute() {
        return Err(ApiError::invalid("Node file paths must be absolute"));
    }
    Ok(Json(
        serde_json::json!({"content":state.host.read_text_file(&request.cwd,&request.path).await.map_err(ApiError::invalid)?}),
    ))
}
async fn write_file(
    State(state): State<WireRuntime>,
    Json(request): Json<FileRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use aow_acp_core::AcpHost;
    if !request.cwd.is_absolute() || !request.path.is_absolute() {
        return Err(ApiError::invalid("Node file paths must be absolute"));
    }
    state
        .host
        .write_text_file(
            &request.cwd,
            &request.path,
            &request
                .content
                .ok_or_else(|| ApiError::invalid("Missing file content"))?,
        )
        .await
        .map_err(ApiError::invalid)?;
    Ok(Json(serde_json::json!({})))
}

#[derive(Deserialize)]
struct WorkspaceFence {
    cwd: PathBuf,
    removing: bool,
}
async fn workspace_removing(
    State(state): State<WireRuntime>,
    Json(request): Json<WorkspaceFence>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !request.cwd.is_absolute() {
        return Err(ApiError::invalid("Workspace path must be absolute"));
    }
    let mut blocked = state.host.filesystem.write().await;
    if request.removing {
        blocked.insert(request.cwd.clone());
    } else {
        blocked.remove(&request.cwd);
    }
    drop(blocked);
    if request.removing {
        let mut instances = state.instances.lock().await;
        let ids: Vec<_> = instances
            .iter()
            .filter(|(_, instance)| instance.spec.cwd == request.cwd)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            instances[id].request_stop();
        }
        for id in ids {
            if let Some(instance) = instances.remove(&id) {
                instance.stop().await;
            }
        }
    }
    Ok(Json(serde_json::json!({})))
}
async fn recover_workspaces(State(state): State<WireRuntime>) -> Json<serde_json::Value> {
    let recovered = state.host.filesystem.read().await.clone();
    let mut instances = state.instances.lock().await;
    let ids: Vec<_> = instances
        .iter()
        .filter(|(_, instance)| recovered.contains(&instance.spec.cwd))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &ids {
        instances[id].request_stop();
    }
    for id in ids {
        if let Some(instance) = instances.remove(&id) {
            instance.stop().await;
        }
    }
    let mut blocked = state.host.filesystem.write().await;
    for cwd in recovered {
        blocked.remove(&cwd);
    }
    Json(serde_json::json!({}))
}

#[derive(Deserialize)]
struct DirectoryRequest {
    path: PathBuf,
}
async fn resolve_directory(
    Json(request): Json<DirectoryRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !request.path.is_absolute() {
        return Err(ApiError::invalid("Node directory path must be absolute"));
    }
    let path = tokio::fs::canonicalize(request.path)
        .await
        .map_err(ApiError::invalid)?;
    if !path.is_dir() {
        return Err(ApiError::invalid("Node working directory must exist"));
    }
    Ok(Json(serde_json::json!({"path":path})))
}

#[derive(Deserialize)]
struct EnvironmentUpdate {
    execution_path: Vec<PathBuf>,
}
async fn save_environment(
    State(state): State<WireRuntime>,
    Json(request): Json<EnvironmentUpdate>,
) -> Result<Json<serde_json::Value>, ApiError> {
    use aow_acp_core::AcpHost;
    let paths = aow_agents_core::environment::normalize_path(request.execution_path)
        .map_err(ApiError::invalid)?;
    let _guard = state.settings_lock.lock().await;
    let path = state.host.root.join("aow-settings.json");
    let mut settings = match tokio::fs::read(&path).await {
        Ok(bytes) => {
            serde_json::from_slice::<serde_json::Value>(&bytes).map_err(ApiError::invalid)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            serde_json::json!({"version":1})
        }
        Err(error) => return Err(ApiError::invalid(error)),
    };
    if !settings.is_object() {
        return Err(ApiError::invalid("Node settings must be an object"));
    }
    settings["execution_path"] = serde_json::to_value(&paths).map_err(ApiError::invalid)?;
    state
        .host
        .write_text_file(&state.host.root, &path, &settings.to_string())
        .await
        .map_err(ApiError::invalid)?;
    Ok(Json(serde_json::json!({"execution_path":paths})))
}
