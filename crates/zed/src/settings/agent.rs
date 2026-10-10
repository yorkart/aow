//! Source: Zed settings_content/src/agent.rs, CustomAgentServerSettings.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum AgentConfigOptionValue {
    ValueId(String),
    Boolean(bool),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum CustomAgentServerSettings {
    Custom {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        default_mode: Option<String>,
        #[serde(default)]
        default_config_options: BTreeMap<String, AgentConfigOptionValue>,
        #[serde(default)]
        favorite_config_option_values: BTreeMap<String, Vec<String>>,
    },
    Registry {
        #[serde(default)]
        env: BTreeMap<String, String>,
        default_mode: Option<String>,
        #[serde(default)]
        default_config_options: BTreeMap<String, AgentConfigOptionValue>,
        #[serde(default)]
        favorite_config_option_values: BTreeMap<String, Vec<String>>,
    },
}

impl CustomAgentServerSettings {
    pub(crate) fn defaults(&self) -> (Option<String>, BTreeMap<String, AgentConfigOptionValue>) {
        match self {
            Self::Custom {
                default_mode,
                default_config_options,
                ..
            }
            | Self::Registry {
                default_mode,
                default_config_options,
                ..
            } => (default_mode.clone(), default_config_options.clone()),
        }
    }
    pub(crate) fn favorites(&self) -> &BTreeMap<String, Vec<String>> {
        match self {
            Self::Custom {
                favorite_config_option_values,
                ..
            }
            | Self::Registry {
                favorite_config_option_values,
                ..
            } => favorite_config_option_values,
        }
    }
}
