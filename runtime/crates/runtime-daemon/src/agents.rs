//! Node-local discovery and launch preparation. Paths are never resolved by the caller.
use anyhow::{Result, ensure};
use aow_agents::launch::{AgentLaunch, AgentRegistration, AgentType};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
struct Node(Arc<PathBuf>);
#[derive(Deserialize)]
struct Profile {
    id: String,
    agent_type: Option<AgentType>,
    display_name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Discover {
    #[serde(default)]
    profiles: Vec<Profile>,
    default_agent_id: Option<String>,
}
#[derive(Deserialize)]
struct Resolve {
    profile: Profile,
}
#[derive(Deserialize)]
struct Prepare {
    launch: AgentLaunch,
    pane_id: String,
    cwd: PathBuf,
    #[serde(default)]
    resume_session_id: Option<String>,
    #[serde(default)]
    managed: bool,
}
type Response<T> = Result<Json<T>, (StatusCode, Json<serde_json::Value>)>;
fn error(error: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"code":"node_agent_error","message":error.to_string()})),
    )
}
pub(crate) fn router(root: PathBuf) -> Router {
    Router::new()
        .route(
            "/v1/environment/discovered-path",
            axum::routing::get(discovered_path),
        )
        .route("/v1/agents/discover", post(discover))
        .route("/v1/agents/resolve", post(resolve))
        .route("/v1/agents/prepare-role", post(prepare_role))
        .route("/v1/agents/prepare-terminal", post(prepare))
        .with_state(Node(Arc::new(root)))
}
async fn paths(node: &Node) -> Result<Vec<PathBuf>> {
    // ACP settings also permit an absent version field on an independently configured node.
    let settings = match tokio::fs::read(node.0.join("aow-settings.json")).await {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(error.into()),
    };
    if let Some(value) = settings
        .get("execution_path")
        .filter(|value| !value.is_null())
    {
        return aow_agents::environment::normalize_path(serde_json::from_value(value.clone())?);
    }
    Ok(aow_agents::environment::discover_path().await)
}
pub(crate) fn executable(command: &str, paths: &[PathBuf]) -> Option<String> {
    let valid = |path: &Path| {
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let command_path = Path::new(command);
    let path = if command_path.components().count() > 1 {
        (command_path.is_absolute() && valid(command_path)).then(|| command_path.to_path_buf())
    } else {
        paths.iter().map(|p| p.join(command)).find(|p| valid(p))
    }?;
    // Relative commands must never depend on the daemon's incidental working directory.
    path.canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}
fn registration(profile: Profile, paths: &[PathBuf], default: Option<&str>) -> AgentRegistration {
    let executable = executable(&profile.command, paths);
    AgentRegistration {
        is_default: default == Some(profile.id.as_str()),
        available: profile.agent_type.is_some() && executable.is_some(),
        id: profile.id,
        agent_type: profile.agent_type,
        display_name: profile.display_name,
        source: "configured".into(),
        command: profile.command,
        executable,
        args: profile.args,
        env: profile.env,
    }
}
async fn discover(
    State(node): State<Node>,
    Json(request): Json<Discover>,
) -> Response<Vec<AgentRegistration>> {
    let paths = paths(&node).await.map_err(error)?;
    let default = request.default_agent_id.as_deref();
    let mut agents: Vec<_> = request
        .profiles
        .into_iter()
        .map(|p| registration(p, &paths, default))
        .collect();
    for kind in AgentType::ALL {
        let definition = kind.agent().definition();
        if agents.iter().any(|a| a.id == definition.id) {
            continue;
        }
        let found = definition
            .commands
            .iter()
            .find_map(|c| executable(c, &paths).map(|p| (*c, p)));
        if found.is_none() && default != Some(definition.id) {
            continue;
        }
        let (command, executable) = found
            .map(|(c, p)| (c, Some(p)))
            .unwrap_or((definition.commands[0], None));
        agents.push(AgentRegistration {
            is_default: default == Some(definition.id),
            id: definition.id.into(),
            agent_type: Some(kind),
            display_name: definition.display_name.into(),
            source: "detected".into(),
            available: executable.is_some(),
            command: command.into(),
            executable,
            args: definition.args.iter().map(|s| (*s).into()).collect(),
            env: BTreeMap::new(),
        });
    }
    Ok(Json(agents))
}
async fn resolve(State(node): State<Node>, Json(request): Json<Resolve>) -> Response<AgentLaunch> {
    let paths = paths(&node).await.map_err(error)?;
    let mut launch = registration(request.profile, &paths, None)
        .into_launch(&paths)
        .map_err(error)?;
    launch
        .env
        .entry("AOW_STATE_DIR".into())
        .or_insert_with(|| node.0.to_string_lossy().into_owned());
    Ok(Json(launch))
}
async fn prepare(
    State(node): State<Node>,
    Json(mut request): Json<Prepare>,
) -> Response<AgentLaunch> {
    async {
        ensure!(
            !request.pane_id.is_empty()
                && request.pane_id.len() <= 128
                && request
                    .pane_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
            "Invalid pane ID"
        );
        ensure!(
            request.cwd.is_absolute() && request.cwd.is_dir(),
            "cwd must exist on this node"
        );
        if let Some(session_id) = &request.resume_session_id {
            request.launch.resume_session(session_id)?;
        }
        if request.managed && request.launch.agent_type == AgentType::Codex {
            let index = request
                .launch
                .args
                .iter()
                .position(|arg| arg == "--")
                .unwrap_or(request.launch.args.len());
            request.launch.args.splice(
                index..index,
                ["-c".into(), "check_for_update_on_startup=false".into()],
            );
        }
        request
            .launch
            .prepare_terminal(&request.pane_id, &request.cwd)
            .await;
        request
            .launch
            .env
            .entry("AOW_STATE_DIR".into())
            .or_insert_with(|| node.0.to_string_lossy().into_owned());
        if request.launch.agent_type == AgentType::Pi {
            aow_agents::pi_bridge::prepare(
                &node.0.join("data/agents/pi"),
                &request.pane_id,
                &mut request.launch.args,
                &mut request.launch.env,
            )?;
        }
        Ok::<_, anyhow::Error>(Json(request.launch))
    }
    .await
    .map_err(error)
}

#[derive(Deserialize)]
struct RoleLaunch {
    launch: AgentLaunch,
    instruction: String,
}
async fn prepare_role(Json(mut request): Json<RoleLaunch>) -> Response<AgentLaunch> {
    aow_agents::role_instruction::prepare_role_instruction(
        &mut request.launch,
        &request.instruction,
    )
    .map_err(self::error)?;
    Ok(Json(request.launch))
}

async fn discovered_path() -> Json<Vec<PathBuf>> {
    Json(aow_agents::environment::discover_path().await)
}
