//! Explicit adapter preparation, separate from ACP connection and session creation.
use super::{AgentServerStore, progress::Progress};
use crate::acp_thread::history::atomic_write;
use crate::facade::ConnectionStatus;
use crate::project::agent_server_store::{AgentServerCommand, get_command};
use crate::settings::validate_settings;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{ffi::OsStr, path::PathBuf};

#[derive(Serialize, Deserialize)]
struct Receipt {
    configuration: String,
    files: Vec<PathBuf>,
}

fn configuration(settings: &Value, id: &str, path: Option<&OsStr>) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{}\n{path:?}", settings["agent_servers"][id]["env"]).as_bytes())
    )
}

impl AgentServerStore {
    fn receipt_path(&self, id: &str) -> PathBuf {
        self.inner
            .cache
            .join("installations")
            .join(format!("{:x}.json", Sha256::digest(id.as_bytes())))
    }

    pub(super) fn installed(&self, id: &str, settings: &Value, path: Option<&OsStr>) -> bool {
        if settings["agent_servers"][id]["type"] == "custom" {
            return true;
        }
        let Ok(bytes) = std::fs::read(self.receipt_path(id)) else {
            return false;
        };
        let Ok(receipt) = serde_json::from_slice::<Receipt>(&bytes) else {
            return false;
        };
        receipt.configuration == configuration(settings, id, path)
            && !receipt.files.is_empty()
            && receipt.files.iter().all(|file| file.is_file())
    }

    pub(super) fn record_installation(
        &self,
        id: &str,
        settings: &Value,
        path: Option<&OsStr>,
        command: &AgentServerCommand,
    ) -> Result<()> {
        if settings["agent_servers"][id]["type"] != "registry" {
            return Ok(());
        }
        let mut files = vec![command.path.clone()];
        // npm adapters use an absolute package entrypoint inside the adapter cache.
        let cache = self.inner.cache.canonicalize()?;
        if let Some(entrypoint) = command.args.first().map(PathBuf::from)
            && entrypoint.starts_with(cache)
        {
            files.push(entrypoint);
        }
        atomic_write(
            &self.receipt_path(id),
            &serde_json::to_vec(&Receipt {
                configuration: configuration(settings, id, path),
                files,
            })?,
        )
    }

    pub async fn install_agent(&self, id: &str) -> Result<()> {
        let progress = Progress::default();
        self.inner
            .installations
            .lock()
            .unwrap()
            .insert(id.into(), progress.clone());
        let result = self.prepare_agent(id, &progress).await;
        progress.finish(result.is_ok());
        result
    }

    async fn prepare_agent(&self, id: &str, progress: &Progress) -> Result<()> {
        let _guard = self.inner.connect_lock.lock().await;
        progress.set("configuration");
        let raw = self.inner.host.settings().await?;
        let settings = validate_settings(&raw)?;
        let values: Value = serde_json_lenient::from_str(&raw)?;
        let agent = settings
            .agent_servers
            .get(id)
            .context("Configure the ACP Agent before installing it")?;
        ensure!(
            values["agent_servers"][id]["type"] == "registry",
            "Custom agents use their configured command and do not need Registry installation"
        );
        let path = self.inner.host.execution_path().await?;
        let command = get_command(
            id,
            agent,
            &self.inner.registry,
            &self.inner.cache.join("agents"),
            path.as_deref(),
            progress,
        )
        .await?;
        self.record_installation(id, &values, path.as_deref(), &command)
    }

    pub fn installation_status(&self, id: &str) -> Option<ConnectionStatus> {
        self.inner
            .installations
            .lock()
            .unwrap()
            .get(id)
            .map(Progress::snapshot)
    }
}
