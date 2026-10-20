mod agent;

pub(crate) use agent::{AgentConfigOptionValue, CustomAgentServerSettings};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;

pub const SETTINGS_PATH: &str = "acp/settings.json";

/// Parsed view only. The settings owner saves the original JSONC text losslessly.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SettingsDocument {
    #[serde(default)]
    pub default_agent_id: Option<String>,
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
    if let Some(id) = &settings.default_agent_id {
        ensure!(
            settings.agent_servers.contains_key(id),
            "默认 ACP 实例不存在: {id}"
        );
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_must_reference_a_configured_adapter_and_is_optional_for_old_settings() {
        assert!(
            validate_settings(r#"{"agent_servers":{"first":{"type":"custom","command":"node"}}}"#)
                .unwrap()
                .default_agent_id
                .is_none()
        );
        assert_eq!(validate_settings(r#"{"default_agent_id":"second","agent_servers":{"first":{"type":"custom","command":"node"},"second":{"type":"registry"}}}"#).unwrap().default_agent_id.as_deref(), Some("second"));
        assert!(validate_settings(r#"{"default_agent_id":"missing","agent_servers":{}}"#).is_err());
    }
}
