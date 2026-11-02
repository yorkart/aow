//! Probe the selected runtime in the candidate role's real home before publishing it.
use super::{AgentRole, RoleRuntime};
use crate::{
    AppState,
    aow::{AowError, AowManager},
};
use anyhow::{Context, Result, bail, ensure};
use aow_acp::AcpService;
use aow_agents::automation::{AgentAutomation, PromptMode};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};

const PROMPT: &str = "ping";

pub(super) async fn test(state: &AppState, role: &AgentRole) -> Result<(), AowError> {
    test_with_timeout(state, role, Duration::from_secs(60)).await
}

pub(super) async fn test_with_timeout(
    state: &AppState,
    role: &AgentRole,
    deadline: Duration,
) -> Result<(), AowError> {
    let home = state.aow.role_home(&role.id)?;
    let workspace = tempfile::Builder::new()
        .prefix("connection-")
        .tempdir_in(home.parent().unwrap())?;
    tokio::time::timeout(deadline, async {
        // Native CLIs can require a repository. This scratch checkout is never
        // registered as a project or exposed as a workspace resource.
        let status = Command::new("git").args(["init", "--quiet"])
            .current_dir(workspace.path()).envs(state.aow.role_environment(role)?)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .kill_on_drop(true).status().await?;
        ensure!(status.success(), "Unable to initialize connection test workspace");
        match &role.config.runtime {
            RoleRuntime::Acp { agent_id } => acp(&state.aow, role, agent_id, workspace.path()).await,
            RoleRuntime::Cli { .. } => cli(&state.aow, role, workspace.path()).await,
        }
    }).await.map_err(|_| AowError::Invalid("Connection test timed out. Check the local login and runtime configuration, then retry.".into()))?
        .map_err(|error| AowError::Invalid(format!("Connection test failed: {error}")))
}

struct ProbeService(AcpService);
impl Drop for ProbeService {
    fn drop(&mut self) {
        // Disconnect on success, failure, timeout and request cancellation.
        for session in self.0.sessions(None) {
            if let Ok(Some(connection)) = self.0.session_connection_info(&session.thread_id) {
                let _ = self.0.disconnect(&connection.id);
            }
        }
    }
}

async fn acp(manager: &AowManager, role: &AgentRole, agent_id: &str, cwd: &Path) -> Result<()> {
    let service = ProbeService(manager.role_acp_service()?);
    ensure!(
        service
            .0
            .agents()
            .await?
            .iter()
            .any(|agent| agent.id == agent_id
                && agent.configured
                && agent.supported
                && agent.installed),
        "Install or configure the selected ACP runtime first"
    );
    let session = service
        .0
        .prepare_session_with_options(
            agent_id,
            cwd.into(),
            aow_acp::SessionCreation {
                execution_environment: Some(manager.role_acp_environment(role)?),
                ..Default::default()
            },
        )
        .await?;
    service.0.resume(&session.thread_id).await?;
    service.0.start_prompt(
        &session.thread_id,
        serde_json::from_value(serde_json::json!([{"type":"text","text":PROMPT}]))?,
    )?;
    loop {
        let snapshot = service.0.snapshot(&session.thread_id)?;
        if let Some(error) = snapshot.error {
            bail!("{error}");
        }
        ensure!(
            snapshot.permissions.is_empty(),
            "The runtime requested permission during the connection test. Complete local setup first."
        );
        match snapshot.status.as_str() {
            "working" => tokio::time::sleep(Duration::from_millis(40)).await,
            "idle" => {
                let reply = snapshot
                    .entries
                    .iter()
                    .filter(|entry| entry.kind == "assistant")
                    .filter_map(|entry| entry.content["text"].as_str())
                    .collect::<String>();
                ensure!(
                    !reply.trim().is_empty(),
                    "The runtime did not return a connection-test response"
                );
                return Ok(());
            }
            status => bail!("Runtime disconnected during connection test ({status})"),
        }
    }
}

struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            // CLI wrappers may leave descendants holding the output pipes.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

async fn limited(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    reader.take(262_145).read_to_end(&mut output).await?;
    ensure!(
        output.len() <= 262_144,
        "Connection test output exceeded its limit"
    );
    Ok(output)
}

async fn cli(manager: &AowManager, role: &AgentRole, cwd: &Path) -> Result<()> {
    let mut probe = role.clone();
    probe.config.instruction.clear();
    let mut launch = manager.role_cli_launch(&probe).await?;
    let mut input = Vec::new();
    let adapter = launch
        .agent_type
        .agent()
        .automation()
        .context("Runtime does not support a connection test")?;
    adapter.validate_arguments(&launch.args)?;
    let id = uuid::Uuid::new_v4().to_string();
    launch
        .args
        .extend(adapter.automation_arguments(false, Some(&id))?);
    match adapter.prompt_mode() {
        PromptMode::Stdin => input = PROMPT.as_bytes().to_vec(),
        PromptMode::Argument(flag) => launch.args.extend([flag.into(), PROMPT.into()]),
    }
    let mut child = Command::new(&launch.executable)
        .args(&launch.args)
        .envs(&launch.env)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()?;
    let _group = ProcessGroup(child.id());
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (_, output, _, status) = tokio::try_join!(
        async {
            stdin.write_all(&input).await?;
            drop(stdin);
            Ok::<_, anyhow::Error>(())
        },
        limited(stdout),
        limited(stderr),
        async { Ok::<_, anyhow::Error>(child.wait().await?) }
    )?;
    ensure!(
        status.success(),
        "Runtime exited with {status}. Check its local login and launch arguments."
    );
    ensure!(
        !String::from_utf8_lossy(&output).trim().is_empty(),
        "The runtime did not return a connection-test response"
    );
    Ok(())
}
