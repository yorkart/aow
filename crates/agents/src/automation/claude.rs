use super::{AgentAutomation, SessionIdMode};

pub(super) struct Claude;

impl AgentAutomation for Claude {
    fn session_id_mode(&self) -> SessionIdMode {
        SessionIdMode::GeneratedUuid
    }
    fn validate_arguments(&self, arguments: &[String]) -> anyhow::Result<()> {
        super::validate_common_arguments(arguments)?;
        for argument in arguments {
            let key = argument.split('=').next().unwrap_or(argument);
            anyhow::ensure!(
                !["-c", "-r", "-p"].iter().any(|flag| key.starts_with(flag)),
                "Claude 启动参数与自动化新会话模式冲突: {key}"
            );
        }
        Ok(())
    }
    fn automation_arguments(
        &self,
        yolo: bool,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        let session_id =
            session_id.ok_or_else(|| anyhow::anyhow!("Claude requires a fresh session UUID"))?;
        let mut args = Vec::new();
        if yolo {
            args.push("--dangerously-skip-permissions".to_owned());
        }
        args.extend([
            "--print".to_owned(),
            "--session-id".to_owned(),
            session_id.to_owned(),
        ]);
        Ok(args)
    }
    fn session_from_line(&self, _line: &[u8]) -> Option<String> {
        None
    }
}
