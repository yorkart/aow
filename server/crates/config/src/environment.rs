//! Server-owned profile overrides and validation of serialized configuration.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};
pub const SETTINGS_FILE: &str = "aow-settings.json";
pub const AGENTS_FILE: &str = "aow-agents.json";
const REGISTRY_VERSION: u32 = 1;

#[derive(Deserialize)]
struct AgentRegistryDocument {
    version: u32,
    items: Vec<RegisteredAgentEnvironment>,
}

#[derive(Deserialize)]
struct RegisteredAgentEnvironment {
    id: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn validate_agent_environment(environment: &BTreeMap<String, String>) -> Result<()> {
    ensure!(
        environment.len() <= 128
            && environment.iter().all(|(key, value)| {
                !key.is_empty()
                    && key.len() <= 256
                    && key
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
                    && value.len() <= 32 * 1024
                    && !value.contains('\0')
            }),
        aow_i18n::message("agents.invalid_agent_environment_configuration", &[])
    );
    Ok(())
}

/// Load the latest registered overrides for one Agent at the start of each run.
/// A missing registry or an Agent without an explicit registration has no overrides.
pub async fn load_agent_environment_from(
    config_dir: &Path,
    agent_id: &str,
) -> Result<BTreeMap<String, String>> {
    let path = config_dir.join(AGENTS_FILE);
    let document: AgentRegistryDocument = match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| {
            aow_i18n::message(
                "agents.unable_to_read_agent_environment_configuration",
                &[("auto0", format!("{}", path.display()))],
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => {
            return Err(error).context(aow_i18n::text(
                "agents.unable_to_read_agent_environment_configuration_2",
            ));
        }
    };
    ensure!(
        document.version == REGISTRY_VERSION,
        aow_i18n::message(
            "agents.unsupported_agent_configuration_version",
            &[("auto0", format!("{}", document.version))]
        )
    );
    let environment = document
        .items
        .into_iter()
        .find(|agent| agent.id == agent_id)
        .map(|agent| agent.env)
        .unwrap_or_default();
    validate_agent_environment(&environment)?;
    Ok(environment)
}

pub fn normalize_path(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    ensure!(
        !paths.is_empty() && paths.len() <= 512,
        aow_i18n::message("agents.path_must_contain_1_512_directories", &[])
    );
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for path in paths {
        let text = path.to_str().context(aow_i18n::text(
            "agents.path_directories_must_be_valid_utf_8",
        ))?;
        ensure!(
            path.is_absolute() && !text.contains(':') && !text.chars().any(char::is_control),
            aow_i18n::message(
                "agents.each_path_entry_must_be_an_absolute_directory_without",
                &[("p0", text.to_string())]
            )
        );
        if seen.insert(path.clone()) {
            normalized.push(path);
        }
    }
    ensure!(
        path_value(&normalized)?.len() <= 64 * 1024,
        aow_i18n::message("agents.path_must_not_exceed_64_kib", &[])
    );
    Ok(normalized)
}

pub fn path_value(paths: &[PathBuf]) -> Result<String> {
    std::env::join_paths(paths)
        .context(aow_i18n::text("agents.unable_to_construct_path"))?
        .into_string()
        .map_err(|_| {
            anyhow::anyhow!(aow_i18n::message(
                "agents.path_directories_must_be_valid_utf_8",
                &[]
            ))
        })
}
