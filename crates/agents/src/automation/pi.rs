use super::{AgentAutomation, SessionIdMode};

pub(super) struct Pi;

impl AgentAutomation for Pi {
    fn session_id_mode(&self) -> SessionIdMode {
        SessionIdMode::GeneratedUuid
    }

    fn validate_arguments(&self, arguments: &[String]) -> anyhow::Result<()> {
        super::validate_common_arguments(arguments)?;
        for argument in arguments {
            let key = argument.split('=').next().unwrap_or(argument);
            anyhow::ensure!(
                ![
                    "--mode",
                    "--session",
                    "--session-dir",
                    "--no-session",
                    "--fork",
                    "--export",
                    "--list-models",
                    "--help",
                    "--version",
                    "--"
                ]
                .contains(&key)
                    && !["-c", "-r", "-p", "-h", "-v"]
                        .iter()
                        .any(|flag| key.starts_with(flag)),
                "Pi 启动参数与自动化新会话模式冲突: {key}"
            );
        }
        Ok(())
    }

    fn automation_arguments(
        &self,
        yolo: bool,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        let id = session_id.ok_or_else(|| anyhow::anyhow!("Pi requires a fresh session UUID"))?;
        // Print mode propagates assistant errors/aborts through the exit status.
        // Pi's approval flag controls project resources, not tool permissions.
        Ok(vec![
            "--print".into(),
            "--session-id".into(),
            id.into(),
            if yolo { "--approve" } else { "--no-approve" }.into(),
        ])
    }

    fn session_from_line(&self, _line: &[u8]) -> Option<String> {
        None
    }
}
