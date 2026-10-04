use std::{io::Write, path::Path, path::PathBuf};

use anyhow::{Context, Result};
use aow_terminald::VtWorkerConfig;
use tempfile::NamedTempFile;

const EMBEDDED_VT_WORKER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vt-worker.mjs"));
const MINIMUM_NODE_MAJOR: u64 = 20;

pub(super) async fn configure(
    node: &Path,
    worker_override: Option<PathBuf>,
) -> (Option<VtWorkerConfig>, Option<NamedTempFile>) {
    match validate_node(node).await {
        Ok(()) => match worker_override {
            Some(worker) => (worker_config_for_override(node.to_path_buf(), worker), None),
            None => match extract_embedded_worker() {
                Ok(worker) => {
                    let config = VtWorkerConfig::new(node.to_path_buf(), worker.path());
                    (Some(config), Some(worker))
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to extract embedded VT worker; using raw terminal replay");
                    (None, None)
                }
            },
        },
        Err(error) => {
            tracing::warn!(
                node = %node.display(),
                %error,
                "Node.js 20+ is unavailable; using raw terminal replay"
            );
            (None, None)
        }
    }
}

fn worker_config_for_override(node: PathBuf, worker: PathBuf) -> Option<VtWorkerConfig> {
    match std::fs::metadata(&worker) {
        Ok(metadata) if metadata.is_file() => Some(VtWorkerConfig::new(node, worker)),
        Ok(_) => {
            tracing::warn!(
                worker = %worker.display(),
                "VT worker override is not a regular file; using raw terminal replay"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                worker = %worker.display(),
                %error,
                "VT worker override is unavailable; using raw terminal replay"
            );
            None
        }
    }
}

async fn validate_node(node: &Path) -> Result<()> {
    let output = tokio::process::Command::new(node)
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("run {} --version", node.display()))?;
    if !output.status.success() {
        anyhow::bail!("Node version command exited with {}", output.status);
    }
    let version = std::str::from_utf8(&output.stdout)
        .context("Node version output is not UTF-8")?
        .trim();
    let major = version
        .strip_prefix('v')
        .unwrap_or(version)
        .split('.')
        .next()
        .context("Node version output is empty")?
        .parse::<u64>()
        .with_context(|| format!("cannot parse Node version {version:?}"))?;
    if major < MINIMUM_NODE_MAJOR {
        anyhow::bail!("Node {version} is older than required v{MINIMUM_NODE_MAJOR}");
    }
    Ok(())
}

fn extract_embedded_worker() -> Result<NamedTempFile> {
    let mut worker = tempfile::Builder::new()
        .prefix("aow-terminald-vt-worker-")
        .suffix(".mjs")
        .tempfile()
        .context("create private temporary VT worker file")?;
    worker
        .write_all(EMBEDDED_VT_WORKER)
        .context("write embedded VT worker")?;
    worker.flush().context("flush embedded VT worker")?;
    Ok(worker)
}

#[cfg(test)]
mod tests {
    use super::validate_node;

    #[tokio::test]
    async fn node_version_validation_requires_node_20_or_newer() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old-node");
        std::fs::write(&old, "#!/bin/sh\nprintf 'v19.9.0\n'\n").unwrap();
        let current = directory.path().join("current-node");
        std::fs::write(&current, "#!/bin/sh\nprintf 'v20.0.0\n'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert!(validate_node(&old).await.is_err());
        validate_node(&current).await.unwrap();
    }
}
