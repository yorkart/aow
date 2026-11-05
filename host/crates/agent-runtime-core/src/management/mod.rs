//! Node-local discovery and launch preparation. Paths are never resolved by the caller.
pub mod probe;
use crate::launch::{AgentLaunch, AgentRegistration, AgentType};
use crate::launch::{AgentLaunchExt, AgentRegistrationExt, AgentTypeExt};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub struct AgentManager(Arc<PathBuf>);

impl AgentManager {
    pub fn new(root: PathBuf) -> Self {
        Self(Arc::new(root))
    }
}
use aow_host_contract::agents::{
    AgentCapabilities, AgentProfile as Profile, DiscoverAgents as Discover, MatchTerminalProfile,
    PrepareRole as RoleLaunch, PrepareRoleSession, PrepareTerminal as Prepare,
    ResolveAgent as Resolve, RoleSessionPlan, ValidateAutomation,
};
async fn paths(node: &AgentManager) -> Result<Vec<PathBuf>> {
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
        return crate::environment::normalize_path(serde_json::from_value(value.clone())?);
    }
    Ok(crate::environment::discover_path().await)
}
pub fn executable(command: &str, paths: &[PathBuf]) -> Option<String> {
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
    let agent_type = profile
        .agent_type
        .or_else(|| AgentType::from_id(&profile.id));
    AgentRegistration {
        is_default: default == Some(profile.id.as_str()),
        available: agent_type.is_some() && executable.is_some(),
        id: profile.id,
        agent_type,
        display_name: profile.display_name,
        source: "configured".into(),
        command: profile.command,
        executable,
        args: profile.args,
        env: profile.env,
    }
}
pub async fn discover(node: &AgentManager, request: Discover) -> Result<Vec<AgentRegistration>> {
    let paths = paths(node).await?;
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
    Ok(agents)
}
pub async fn resolve(node: &AgentManager, request: Resolve) -> Result<AgentLaunch> {
    let paths = paths(node).await?;
    let mut launch = registration(request.profile, &paths, None).into_launch(&paths)?;
    launch.env.insert(
        "AOW_STATE_DIR".into(),
        node.0.to_string_lossy().into_owned(),
    );
    Ok(launch)
}
pub async fn prepare(node: &AgentManager, mut request: Prepare) -> Result<AgentLaunch> {
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
            // Rebuild preparation is idempotent; remove the previously injected override.
            let mut index = 0;
            while index + 1 < request.launch.args.len() && request.launch.args[index] != "--" {
                if request.launch.args[index] == "-c"
                    && request.launch.args[index + 1] == "check_for_update_on_startup=false"
                {
                    request.launch.args.drain(index..index + 2);
                } else {
                    index += 1;
                }
            }
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
        request.launch.env.insert(
            "AOW_STATE_DIR".into(),
            node.0.to_string_lossy().into_owned(),
        );
        if request.launch.agent_type == AgentType::Pi {
            crate::pi_bridge::prepare(
                &node.0.join("data/agents/pi"),
                &request.pane_id,
                &mut request.launch.args,
                &mut request.launch.env,
            )?;
        }
        Ok::<_, anyhow::Error>(request.launch)
    }
    .await
}

pub async fn prepare_role(mut request: RoleLaunch) -> Result<AgentLaunch> {
    crate::role_instruction::prepare_role_instruction(&mut request.launch, &request.instruction)?;
    Ok(request.launch)
}

pub async fn discovered_path() -> Vec<PathBuf> {
    crate::environment::discover_path().await
}

pub async fn capabilities() -> Vec<AgentCapabilities> {
    crate::KNOWN_AGENTS
        .iter()
        .map(|agent| {
            let definition = agent.definition();
            AgentCapabilities {
                id: agent.id().into(),
                display_name: definition.display_name.into(),
                commands: definition.commands.iter().map(|s| (*s).into()).collect(),
                configuration_env: definition
                    .configuration_env
                    .iter()
                    .map(|s| (*s).into())
                    .collect(),
                interactive: agent.interactive().is_some(),
                automation: agent.automation().is_some(),
                sessions: agent.sessions().is_some(),
            }
        })
        .collect()
}
pub async fn validate_automation(request: ValidateAutomation) -> Result<()> {
    use crate::automation::AgentAutomation;
    request.agent.validate_arguments(&request.arguments)?;
    Ok(())
}
pub async fn match_terminal(request: MatchTerminalProfile) -> Vec<String> {
    request
        .profiles
        .into_iter()
        .filter(|profile| {
            profile.available
                && profile
                    .agent_type
                    .is_some_and(|kind| kind.id() == request.agent)
                && profile.executable.as_deref() == Some(&request.executable)
                && profile.matches_terminal_arguments(&request.arguments)
        })
        .map(|profile| profile.id)
        .collect()
}
pub async fn prepare_role_session(request: PrepareRoleSession) -> Result<RoleSessionPlan> {
    let mut plan = RoleSessionPlan {
        launch: request.launch,
        initial_arguments: vec![],
        task: request.prompt,
        managed: false,
    };
    if plan.launch.agent_type == AgentType::Claude {
        if let Some(prompt) = plan.task.take() {
            if !plan.launch.args.iter().any(|arg| arg == "--") {
                plan.initial_arguments.push("--".into());
            }
            plan.initial_arguments.push(prompt);
        }
    } else {
        if matches!(
            plan.launch.agent_type,
            AgentType::Hermes | AgentType::TraeCli
        ) && !request.instruction.trim().is_empty()
        {
            plan.task = Some(match plan.task {
                Some(prompt) => format!("{}\n\n{prompt}", request.instruction),
                None => request.instruction,
            });
        }
        plan.managed = plan.task.is_some();
        if plan.managed {
            ensure_interactive(plan.launch.agent_type.id())?;
        }
    }
    Ok(plan)
}
fn ensure_interactive(id: &str) -> Result<()> {
    ensure!(
        crate::Agent::from_id(id)
            .and_then(crate::Agent::interactive)
            .is_some(),
        "Agent does not support interactive startup"
    );
    Ok(())
}

pub async fn configuration_environment() -> BTreeMap<String, String> {
    crate::automation::configuration_environment()
}

pub async fn role_layout(
    request: aow_host_contract::agents::ResolveRoleLayout,
) -> Result<aow_host_environment::HomeLayout> {
    Ok(aow_host_environment::role_layout(&request.agent_id)?)
}
