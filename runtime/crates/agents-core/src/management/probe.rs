use super::*;
use crate::automation::{AgentAutomation, PromptMode};
use anyhow::Context;
use aow_runtime_contract::agents::ProbeAgent;

pub async fn run(node: &AgentManager, request: ProbeAgent) -> Result<()> {
    async {
        ensure!(
            (1..=120_000).contains(&request.timeout_ms),
            "Invalid probe timeout"
        );
        tokio::time::timeout(
            std::time::Duration::from_millis(request.timeout_ms),
            async {
                let workspace = tempfile::Builder::new()
                    .prefix("connection-")
                    .tempdir_in(node.0.as_ref())?;
                let result =
                    aow_workspaces::command(workspace.path(), &["init".into(), "--quiet".into()])
                        .await?;
                ensure!(
                    result.success,
                    "Unable to initialize connection test workspace"
                );
                cli(request.launch, workspace.path(), request.timeout_ms).await
            },
        )
        .await
        .context("Connection test timed out")??;
        Ok(())
    }
    .await
}
pub async fn cli(mut launch: AgentLaunch, cwd: &Path, timeout_ms: u64) -> Result<()> {
    let adapter = launch
        .agent_type
        .agent()
        .automation()
        .context("Unsupported connection test")?;
    adapter.validate_arguments(&launch.args)?;
    launch
        .args
        .extend(adapter.automation_arguments(false, Some(&uuid::Uuid::new_v4().to_string()))?);
    let input = match adapter.prompt_mode() {
        PromptMode::Stdin => b"ping".to_vec(),
        PromptMode::Argument(flag) => {
            launch.args.extend([flag.into(), "ping".into()]);
            Vec::new()
        }
    };
    let result = aow_process::run_once(aow_process::ProcessSpec {
        executable: launch.executable.into(),
        arguments: launch.args,
        cwd: cwd.into(),
        environment: launch.env,
        stdin: input,
        timeout_ms,
        max_output_bytes: 262144,
    })
    .await?;
    ensure!(result.success, "Runtime exited with {:?}", result.exit_code);
    ensure!(
        !String::from_utf8_lossy(&result.stdout).trim().is_empty(),
        "Runtime did not return a connection-test response"
    );
    Ok(())
}
