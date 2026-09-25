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
        "Agent 环境变量配置无效"
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
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("无法读取 Agent 环境变量配置：{}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error).context("无法读取 Agent 环境变量配置"),
    };
    ensure!(
        document.version == REGISTRY_VERSION,
        "不支持的 Agent 配置版本：{}",
        document.version
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
        "PATH 必须包含 1–512 个目录"
    );
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for path in paths {
        let text = path.to_str().context("PATH 目录必须是有效的 UTF-8")?;
        ensure!(
            path.is_absolute() && !text.contains(':') && !text.chars().any(char::is_control),
            "PATH 每项必须是绝对目录路径，不能包含冒号或控制字符：{text}"
        );
        ensure!(
            !path.is_file(),
            "PATH 应填写目录，不能填写可执行文件：{text}"
        );
        if seen.insert(path.clone()) {
            normalized.push(path);
        }
    }
    ensure!(
        path_value(&normalized)?.len() <= 64 * 1024,
        "PATH 不能超过 64 KiB"
    );
    Ok(normalized)
}

pub fn path_value(paths: &[PathBuf]) -> Result<String> {
    std::env::join_paths(paths)
        .context("无法生成 PATH")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("PATH 目录必须是有效的 UTF-8"))
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
/// Missing settings (including pre-upgrade installations) use local discovery;
/// malformed settings fail explicitly instead of silently selecting another runtime.
pub async fn load_path(state_dir: &Path) -> Result<Vec<PathBuf>> {
    load_path_from(&aow_config::configuration_directory(state_dir)?).await
}

/// Use the same configuration selection as the task loaded by the runner.
pub async fn load_path_from(config_dir: &Path) -> Result<Vec<PathBuf>> {
    let path = config_dir.join(SETTINGS_FILE);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let document: SettingsDocument = serde_json::from_slice(&bytes)
                .with_context(|| format!("无法读取执行环境配置：{}", path.display()))?;
            ensure!(
                document.version == 1,
                "不支持的 AoW 配置版本：{}",
                document.version
            );
            if let Some(paths) = document.execution_path {
                return normalize_path(paths);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("无法读取执行环境配置"),
    }
    normalize_path(discover_path().await)
}
