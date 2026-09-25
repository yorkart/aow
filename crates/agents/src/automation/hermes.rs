use super::{AgentAutomation, PromptMode, SessionIdMode};

pub(super) struct Hermes;

impl AgentAutomation for Hermes {
    fn session_id_mode(&self) -> SessionIdMode {
        SessionIdMode::FromStderrOnExit
    }

    fn prompt_mode(&self) -> PromptMode {
        PromptMode::Argument("--query")
    }

    fn validate_arguments(&self, arguments: &[String]) -> anyhow::Result<()> {
        super::validate_common_arguments(arguments)?;
        for argument in arguments {
            let key = argument.split('=').next().unwrap_or(argument);
            anyhow::ensure!(
                !["chat", "--query", "--oneshot", "--source", "--tui", "--"].contains(&key)
                    && !["-q", "-z", "-r", "-c"]
                        .iter()
                        .any(|flag| key.starts_with(flag)),
                "Hermes 启动参数与自动化新会话模式冲突: {key}"
            );
        }
        Ok(())
    }

    fn automation_arguments(
        &self,
        yolo: bool,
        _session_id: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        let mut args = vec!["chat".into(), "--cli".into(), "--quiet".into()];
        if yolo {
            args.push("--yolo".into());
        }
        Ok(args)
    }

    fn session_from_line(&self, line: &[u8]) -> Option<String> {
        let id = std::str::from_utf8(line)
            .ok()?
            .trim()
            .strip_prefix("session_id:")?
            .trim();
        (!id.is_empty()
            && id.len() <= 256
            && !id.starts_with('-')
            && id
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b"_-".contains(&ch)))
        .then(|| id.to_owned())
    }
}
