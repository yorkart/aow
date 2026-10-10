//! Zed node_runtime system-runtime discovery and npm package entrypoint resolution.
//! AoW supplies PATH; runtime discovery, installation and cache stay inside this module.

use crate::agent_servers::progress::Progress;
use anyhow::{Context, Result, ensure};
use semver::Version;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, BufReader};

pub(crate) struct NodeRuntime {
    pub node: PathBuf,
    npm: PathBuf,
    pub execution_path: OsString,
}

impl NodeRuntime {
    pub(crate) async fn discover(
        host_path: Option<&OsStr>,
        environment: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let path = environment
            .get("PATH")
            .map(OsString::from)
            .or_else(|| host_path.map(OsStr::to_os_string))
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default();
        let node = find_executable("node", &path).context(
            "ACP Registry npm adapters require Node.js 22 or newer. Add its bin directory in Settings → Environment execution PATH",
        )?;
        // AoW's launcher exposes a node symlink without npm/npx alongside it.
        // Resolve the selected runtime and use npm from that same installation.
        let node = std::fs::canonicalize(node)?;
        let binary_directory = node.parent().context("Node.js has no parent directory")?;
        let npm = executable(&binary_directory.join("npm"))
            .or_else(|| find_executable("npm", &path))
            .with_context(|| format!(
                "ACP Registry npm adapters require npm alongside Node.js ({}). Install npm or add its bin directory in Settings → Environment execution PATH",
                node.display()
            ))?;
        let execution_path = std::env::join_paths(
            std::iter::once(binary_directory.to_owned()).chain(std::env::split_paths(&path)),
        )?;
        let output = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new(&node)
                .arg("--version")
                .envs(environment)
                .env("PATH", &execution_path)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("Node.js version check timed out")??;
        ensure!(
            output.status.success(),
            "Node.js version check failed for {}",
            node.display()
        );
        let version = Version::parse(
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .trim_start_matches('v'),
        )
        .context("Invalid Node.js version")?;
        ensure!(
            version >= Version::new(22, 0, 0) && version.pre.is_empty(),
            "ACP Registry npm adapters require Node.js 22 or newer; {} is {version}",
            node.display()
        );
        Ok(Self {
            node,
            npm,
            execution_path,
        })
    }

    pub(crate) async fn install_package(
        &self,
        directory: &Path,
        package_name: &str,
        package_spec: &str,
        environment: &BTreeMap<String, String>,
        progress: &Progress,
    ) -> Result<PathBuf> {
        ensure!(
            valid_package_name(package_name),
            "Invalid Registry npm package name: {package_name}"
        );
        tokio::fs::create_dir_all(directory).await?;
        let directory = tokio::fs::canonicalize(directory).await?;
        let marker = directory.join(".aow-package-spec");
        if tokio::fs::read_to_string(&marker).await.ok().as_deref() == Some(package_spec)
            && let Ok(executable) =
                read_package_executable(&directory.join("node_modules"), package_name).await
        {
            return Ok(executable);
        }
        // Initialize an isolated npm project; never let npm select the user's workspace root.
        progress.set("installing");
        tokio::fs::write(directory.join("package.json"), b"{\"private\":true}\n").await?;
        let mut child = tokio::process::Command::new(&self.npm)
            .arg("--prefix")
            .arg(&directory)
            .arg("--cache")
            .arg(directory.join(".npm-cache"))
            .args([
                "install",
                "--save-exact",
                "--no-audit",
                "--no-fund",
                "--loglevel=http",
                "--",
                package_spec,
            ])
            .current_dir(&directory)
            .envs(environment)
            .env("PATH", &self.execution_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to start npm at {}", self.npm.display()))?;
        let stderr = child.stderr.take().context("Missing npm stderr")?;
        let (output, stderr) = tokio::time::timeout(
            Duration::from_secs(300),
            async { tokio::try_join!(child.wait_with_output(), read_install_output(stderr, progress)) },
        )
        .await
        .context("ACP npm installation timed out after 5 minutes. Check Settings → Environment server.env proxy settings and reload the service")??;
        ensure!(
            output.status.success(),
            "ACP npm installation failed for {package_name} ({}): {}",
            output.status,
            stderr
        );
        let executable =
            read_package_executable(&directory.join("node_modules"), package_name).await?;
        tokio::fs::write(marker, package_spec).await?;
        Ok(executable)
    }
}

async fn read_install_output(
    stderr: tokio::process::ChildStderr,
    progress: &Progress,
) -> std::io::Result<String> {
    let mut lines = BufReader::new(stderr).lines();
    let mut tail = std::collections::VecDeque::new();
    while let Some(line) = lines.next_line().await? {
        for code in [
            "ETIMEDOUT",
            "ECONNRESET",
            "ENOTFOUND",
            "ECONNREFUSED",
            "EAI_AGAIN",
        ] {
            if line.contains(code) {
                progress.network_error(code);
                break;
            }
        }
        tail.push_back(line.chars().take(2048).collect::<String>());
        if tail.len() > 32 {
            tail.pop_front();
        }
    }
    Ok(tail.into_iter().collect::<Vec<_>>().join("\n"))
}

fn executable(path: &Path) -> Option<PathBuf> {
    let metadata = std::fs::metadata(path).ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then(|| path.to_owned())
}

fn find_executable(name: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path).find_map(|directory| executable(&directory.join(name)))
}

fn valid_package_name(name: &str) -> bool {
    let segments: Vec<_> = name.split('/').collect();
    let parts = if name.starts_with('@') && segments.len() == 2 {
        vec![&segments[0][1..], segments[1]]
    } else if !name.starts_with('@') && segments.len() == 1 {
        segments
    } else {
        return false;
    };
    parts.iter().all(|part| {
        !part.is_empty()
            && !part.starts_with(['.', '-'])
            && part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    })
}

// Preserve the upstream version ceiling so npm min-release-age policy is respected.
pub(crate) fn bounded_npm_package_spec(package_spec: &str) -> (&str, String) {
    let Some((package_name, version)) = package_spec.rsplit_once('@') else {
        return (package_spec, package_spec.to_string());
    };
    if package_name.is_empty() {
        return (package_spec, package_spec.to_string());
    }
    if Version::parse(version).is_err() {
        return (package_name, package_spec.to_string());
    }
    (package_name, format!("{package_name}@0.0.0 - {version}"))
}

pub(crate) async fn read_package_executable(node_modules: &Path, name: &str) -> Result<PathBuf> {
    let package_directory = node_modules.join(name);
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Bin {
        Path(String),
        Named(BTreeMap<String, String>),
    }
    #[derive(Deserialize)]
    struct PackageJson {
        bin: Option<Bin>,
    }
    let manifest = package_directory.join("package.json");
    let package: PackageJson = serde_json::from_slice(
        &tokio::fs::read(&manifest)
            .await
            .with_context(|| format!("Opening {}", manifest.display()))?,
    )?;
    let relative = match package.bin {
        Some(Bin::Path(path)) => path,
        Some(Bin::Named(bins)) => {
            let unscoped_name = name.rsplit('/').next().unwrap_or(name);
            if bins.len() == 1 {
                bins.values().next()
            } else {
                bins.get(unscoped_name)
            }
            .with_context(|| {
                format!("npm package {name} declares no executable named {unscoped_name}")
            })?
            .clone()
        }
        None => anyhow::bail!("npm package {name} declares no executable"),
    };
    let directory = tokio::fs::canonicalize(package_directory).await?;
    let executable = tokio::fs::canonicalize(directory.join(relative)).await?;
    ensure!(
        executable.starts_with(&directory) && executable.is_file(),
        "Invalid npm package executable for {name}"
    );
    Ok(executable)
}

#[cfg(test)]
mod tests;
