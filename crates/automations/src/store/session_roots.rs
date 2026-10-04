use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf, process::Command};

use anyhow::{Context, Result};
use aow_agents::sessions::SessionRoots;

use super::{Store, atomic_write};

const FILE: &str = "session-roots.json";

impl Store {
    /// Freeze only native history paths, never credentials or the full environment.
    /// Read the command after all launch/registered overrides have been applied.
    pub(crate) fn capture_session_roots(
        &self,
        task_id: &str,
        run_id: &str,
        command: &Command,
    ) -> Result<()> {
        let inherited_cwd = std::env::current_dir()?;
        let cwd = command.get_current_dir().unwrap_or(&inherited_cwd);
        let overrides = command.get_envs().collect::<BTreeMap<_, _>>();
        let mut environment = std::iter::once("HOME")
            .chain(
                aow_agents::KNOWN_AGENTS
                    .iter()
                    .flat_map(|agent| agent.definition().configuration_env.iter().copied()),
            )
            .filter_map(|key| {
                let value = match overrides.get(OsStr::new(key)) {
                    Some(value) => value.map(|value| value.to_os_string()),
                    None => std::env::var_os(key),
                }?;
                if value.is_empty() {
                    return None;
                }
                Some((key.to_owned(), cwd.join(value)))
            })
            .collect::<BTreeMap<String, PathBuf>>();
        // Resolve the fallback once against the actual command environment.
        if !environment.contains_key("HOME")
            && let Some(home) = std::env::home_dir().filter(|home| !home.as_os_str().is_empty())
        {
            environment.insert("HOME".into(), cwd.join(home));
        }
        let home = environment.get("HOME").context("无法确定 Agent 的 HOME")?;
        // This also freezes native indirection such as Hermes active_profile.
        let roots = SessionRoots::from_configuration(home, &environment);
        atomic_write(
            &self.run_path(task_id, run_id)?.join(FILE),
            &serde_json::to_vec(&roots)?,
        )
    }

    pub fn run_session_roots(&self, task_id: &str, run_id: &str) -> Result<SessionRoots> {
        let bytes = std::fs::read(self.run_path(task_id, run_id)?.join(FILE))
            .context("本次运行缺少会话目录记录")?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}
