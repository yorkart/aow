use std::{collections::BTreeMap, path::Path, process::Stdio};

use anyhow::Result;
use aow_agents::automation::{AgentAutomation, PromptMode};
use tokio::process::Command;

use crate::{AgentKind, Task};

pub fn validate_arguments(kind: AgentKind, arguments: &[String]) -> Result<()> {
    kind.validate_arguments(arguments)
}

pub fn command(
    task: &Task,
    agent_environment: &BTreeMap<String, String>,
    directory: &Path,
    session_id: Option<&str>,
) -> Result<Command> {
    validate_arguments(task.input.agent, &task.launch.args)?;
    let mut command = Command::new(&task.launch.executable);
    command
        .args(&task.launch.args)
        .current_dir(directory)
        .envs(&task.launch.environment)
        .envs(agent_environment)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    if task.input.agent.agent() == aow_agents::Agent::Pi {
        // A scheduled process must not inherit the launching terminal's binding.
        command.env_remove("AOW_PI_BINDING");
    }
    command
        .args(
            task.input
                .agent
                .automation_arguments(task.input.yolo, session_id)?,
        )
        .stdout(Stdio::piped());
    match task.input.agent.prompt_mode() {
        PromptMode::Stdin => {
            command.stdin(Stdio::piped());
        }
        PromptMode::Argument(flag) => {
            // Keep leading dashes in the prompt from becoming CLI options.
            command
                .arg(format!("{flag}={}", task.input.prompt))
                .stdin(Stdio::null());
        }
    }
    Ok(command)
}
