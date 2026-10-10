mod agent;

pub(crate) use agent::{AgentConfigOptionValue, CustomAgentServerSettings};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;

pub const SETTINGS_PATH: &str = "zed/settings.json";

/// Parsed view only. The settings owner saves the original JSONC text losslessly.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SettingsDocument {
    #[serde(default)]
    pub(crate) agent_servers: BTreeMap<String, CustomAgentServerSettings>,
}

pub fn validate_settings(text: &str) -> Result<SettingsDocument> {
    let value: serde_json::Value =
        serde_json_lenient::from_str(text).context("ACP JSONC 配置无效")?;
    ensure!(value.is_object(), "ACP 配置必须是 JSON 对象");
    let settings: SettingsDocument = serde_json::from_value(value)?;
    for (id, agent) in &settings.agent_servers {
        ensure!(
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                && id != "."
                && id != "..",
            "无效的 Agent ID: {id}"
        );
        if let CustomAgentServerSettings::Custom { command, .. } = agent {
            ensure!(!command.trim().is_empty(), "Agent {id} 缺少 command");
        }
    }
    Ok(settings)
}
