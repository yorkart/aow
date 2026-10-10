//! Source: Zed project/src/agent_server_store.rs. Registry command resolution and installation.
use super::agent_registry_store::{AgentRegistryStore, RegistryTargetConfig, current_platform_key};
use crate::agent_servers::progress::Progress;
use crate::node_runtime::{NodeRuntime, bounded_npm_package_spec};
use crate::settings::CustomAgentServerSettings;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub(crate) struct AgentServerCommand {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

pub(crate) async fn get_command(
    id: &str,
    settings: &CustomAgentServerSettings,
    registry: &AgentRegistryStore,
    cache: &Path,
    execution_path: Option<&std::ffi::OsStr>,
    progress: &Progress,
) -> Result<AgentServerCommand> {
    match settings {
        CustomAgentServerSettings::Custom {
            command, args, env, ..
        } => Ok(AgentServerCommand {
            path: command.into(),
            args: args.clone(),
            env: env.clone(),
        }),
        CustomAgentServerSettings::Registry { env, .. } => {
            progress.set("registry");
            let entries = registry.agents(false).await?;
            let entry = entries
                .iter()
                .find(|entry| entry.id == id)
                .context("Agent not found in ACP Registry")?;
            let target =
                current_platform_key().and_then(|key| entry.distribution.binary.as_ref()?.get(key));
            if let Some(target) = target {
                let version = format!("{:x}", Sha256::digest(entry.version.as_bytes()));
                let directory = cache.join(id).join(version);
                let relative = target.cmd.trim_start_matches("./");
                ensure!(
                    safe_relative(Path::new(relative)),
                    "Invalid Registry executable path"
                );
                let executable = directory.join(relative);
                if !executable.is_file() {
                    install_binary(target, &directory, progress).await?;
                }
                ensure!(
                    executable.is_file(),
                    "Registry executable missing: {}",
                    executable.display()
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
                }
                let mut environment = target.env.clone();
                environment.extend(env.clone());
                Ok(AgentServerCommand {
                    path: std::fs::canonicalize(executable)?,
                    args: target.args.clone(),
                    env: environment,
                })
            } else if let Some(npx) = &entry.distribution.npx {
                let mut environment = npx.env.clone();
                environment.extend(env.clone());
                progress.set("runtime");
                let runtime = NodeRuntime::discover(execution_path, &environment).await?;
                let (package_name, package_spec) = bounded_npm_package_spec(&npx.package);
                let version = format!("{:x}", Sha256::digest(npx.package.as_bytes()));
                let directory = cache.join(id).join("npm").join(version);
                let executable = runtime
                    .install_package(
                        &directory,
                        package_name,
                        &package_spec,
                        &environment,
                        progress,
                    )
                    .await?;
                let mut args = vec![executable.to_string_lossy().into_owned()];
                args.extend(npx.args.clone());
                environment.insert(
                    "PATH".into(),
                    runtime.execution_path.to_string_lossy().into_owned(),
                );
                Ok(AgentServerCommand {
                    path: runtime.node,
                    args,
                    env: environment,
                })
            } else {
                anyhow::bail!("Agent does not support this platform")
            }
        }
    }
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

async fn install_binary(
    target: &RegistryTargetConfig,
    directory: &Path,
    progress: &Progress,
) -> Result<()> {
    progress.set("download");
    let response = reqwest::Client::new()
        .get(&target.archive)
        .timeout(Duration::from_secs(120))
        .send()
        .await?
        .error_for_status()?;
    ensure!(
        response.content_length().unwrap_or(0) <= 256 * 1024 * 1024,
        "ACP archive too large"
    );
    let bytes = response.bytes().await?;
    ensure!(bytes.len() <= 256 * 1024 * 1024, "ACP archive too large");
    if let Some(expected) = &target.sha256 {
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(expected),
            "ACP archive checksum mismatch"
        );
    }
    let directory = directory.to_owned();
    progress.set("extract");
    let zip = target
        .archive
        .split('?')
        .next()
        .unwrap_or("")
        .ends_with(".zip");
    tokio::task::spawn_blocking(move || -> Result<()> {
        let staging = directory.with_extension(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&staging)?;
        let result = (|| -> Result<()> {
            if zip {
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
                for index in 0..archive.len() {
                    let mut entry = archive.by_index(index)?;
                    let relative = entry.enclosed_name().context("Invalid ACP archive path")?;
                    ensure!(!entry.is_symlink(), "ACP archive links are unsupported");
                    let output = staging.join(relative);
                    if entry.is_dir() {
                        std::fs::create_dir_all(output)?;
                    } else {
                        std::fs::create_dir_all(output.parent().context("Invalid archive path")?)?;
                        std::io::copy(&mut entry, &mut std::fs::File::create(output)?)?;
                    }
                }
            } else {
                let mut archive =
                    tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)));
                for entry in archive.entries()? {
                    let mut entry = entry?;
                    ensure!(safe_relative(&entry.path()?), "Invalid ACP archive path");
                    ensure!(
                        entry.header().entry_type().is_file()
                            || entry.header().entry_type().is_dir(),
                        "ACP archive links are unsupported"
                    );
                    ensure!(entry.unpack_in(&staging)?, "Invalid ACP archive path");
                }
            }
            if directory.exists() {
                std::fs::remove_dir_all(&directory)?;
            }
            std::fs::rename(&staging, &directory)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(staging);
        }
        result
    })
    .await?
}
