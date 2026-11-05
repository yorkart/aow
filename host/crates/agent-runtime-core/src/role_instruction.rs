//! Native instruction flags are execution-node adapter details.
use crate::launch::{AgentLaunch, AgentType};
pub fn prepare_role_instruction(launch: &mut AgentLaunch, instruction: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        instruction.len() <= 131072 && !instruction.contains('\0'),
        "Invalid role instruction"
    );
    if instruction.trim().is_empty() {
        return Ok(());
    }
    let extra = match launch.agent_type {
        AgentType::Codex => vec![
            "-c".into(),
            format!(
                "developer_instructions={}",
                serde_json::to_string(instruction)?
            ),
        ],
        AgentType::Claude | AgentType::Pi => {
            vec!["--append-system-prompt".into(), instruction.into()]
        }
        _ => Vec::new(),
    };
    let index = launch
        .args
        .iter()
        .position(|argument| argument == "--")
        .unwrap_or(launch.args.len());
    launch.args.splice(index..index, extra);
    Ok(())
}
