//! Shared execution environment for interactive agents and background runners.
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use tokio::process::Command;

pub const SETTINGS_FILE: &str = "aow-settings.json";
pub const AGENTS_FILE: &str = "aow-agents.json";
const REGISTRY_VERSION: u32 = 1;

/// Only read the shared part of Settings; other settings belong to the server.
#[derive(Deserialize)]
struct SettingsDocument {
    version: u32,
    #[serde(default)]
    execution_path: Option<Vec<PathBuf>>,
}

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
        ensure!(
            !path.is_file(),
            aow_i18n::message(
                "agents.path_must_contain_directories_not_executable_files",
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

/// Discover defaults only. Saved Settings always take precedence over discovery.
pub async fn discover_path() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(shell) = std::env::var_os("SHELL") {
        let mut command = Command::new(shell);
        command
            // Mark the value so output from shell startup files is not mistaken for PATH.
            .args(["-lc", "printf '\\0%s\\0' \"$PATH\""])
            .stdin(Stdio::null())
            .kill_on_drop(true);
        if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(3), command.output()).await
            && output.status.success()
            && let Some(value) = output.stdout.split(|byte| *byte == 0).nth(1)
            && let Ok(value) = std::str::from_utf8(value)
        {
            paths.extend(std::env::split_paths(value));
        }
    }
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        paths.extend([
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            home.join("bin"),
        ]);
    }
    paths.extend(["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    let mut seen = HashSet::new();
    paths.retain(|path| {
        path.is_absolute()
            && path
                .to_str()
                .is_some_and(|value| !value.contains(':') && !value.chars().any(char::is_control))
            && !path.is_file()
            && seen.insert(path.clone())
    });
    paths
}

/// Reload once per execution, including when the Web service is offline.
/// Missing settings use local discovery;
/// malformed settings fail explicitly instead of silently selecting another runtime.
pub async fn load_path(state_dir: &Path) -> Result<Vec<PathBuf>> {
    load_path_from(state_dir).await
}

/// Read from the same application home as the task loaded by the runner.
pub async fn load_path_from(config_dir: &Path) -> Result<Vec<PathBuf>> {
    let path = config_dir.join(SETTINGS_FILE);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let document: SettingsDocument = serde_json::from_slice(&bytes).with_context(|| {
                aow_i18n::message(
                    "agents.unable_to_read_execution_environment_configuration",
                    &[("auto0", format!("{}", path.display()))],
                )
            })?;
            ensure!(
                document.version == 1,
                aow_i18n::message(
                    "agents.unsupported_aow_configuration_version",
                    &[("auto0", format!("{}", document.version))]
                )
            );
            if let Some(paths) = document.execution_path {
                return normalize_path(paths);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).context(aow_i18n::text(
                "agents.unable_to_read_execution_environment_configuration_2",
            ));
        }
    }
    normalize_path(discover_path().await)
}
