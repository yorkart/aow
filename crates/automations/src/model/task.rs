use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::AgentKind;
use aow_workspaces::{WorkspaceConfig, WorkspaceMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureNotification {
    Feishu,
    Wechat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    #[default]
    Scheduled,
    Manual,
}

/// Editor-confirmed replacement, with UTF-8 byte offsets into the saved prompt.
/// Execution validates these bindings but never discovers additional variables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBinding {
    pub name: String,
    pub placeholder: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskInput {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub kind: TaskKind,
    #[serde(default)]
    pub prompt_bindings: Vec<PromptBinding>,
    pub agent: AgentKind,
    /// Saved configuration ID; older tasks select the built-in configuration for their type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub project_id: String,
    #[serde(flatten)]
    pub workspace: WorkspaceConfig,
    /// Legacy presentation field retained for saved task compatibility.
    ///
    /// Per-run Worktrees are always removed after execution; the runner does
    /// not use this value to decide whether cleanup occurs.
    #[serde(default = "default_cleanup_worktree")]
    pub cleanup_worktree: bool,
    /// Five-field numeric cron, evaluated in the machine's local timezone.
    #[serde(default)]
    pub cron: String,
    /// Native timer interval; None preserves existing calendar schedules.
    #[serde(default)]
    pub interval_seconds: Option<u64>,
    /// Maximum number of this task's runs that may execute at once.
    pub max_concurrent_runs: u8,
    pub enabled: bool,
    /// Run the agent with its native Full Access / Yolo switch.
    #[serde(default = "default_yolo")]
    pub yolo: bool,
    /// Delivery preference only. The server handles notifications independently.
    #[serde(default)]
    pub failure_notification: Option<FailureNotification>,
}

fn default_cleanup_worktree() -> bool {
    true
}

fn default_yolo() -> bool {
    true
}

impl TaskInput {
    pub fn agent_id(&self) -> &str {
        self.agent_id.as_deref().unwrap_or_else(|| self.agent.id())
    }

    pub fn validate_schedule(&self) -> Result<()> {
        if self.kind == TaskKind::Manual {
            ensure!(
                self.cron.is_empty() && self.interval_seconds.is_none(),
                aow_i18n::message("automations.manual_tasks_do_not_require_a_schedule", &[])
            );
            return Ok(());
        }
        if let Some(seconds) = self.interval_seconds {
            ensure!(
                (1..=2_678_400).contains(&seconds),
                aow_i18n::message(
                    "automations.run_interval_must_be_between_1_second_and_31",
                    &[]
                )
            );
        } else {
            crate::Schedule::parse(&self.cron)?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 200,
            aow_i18n::message(
                "automations.name_must_not_be_empty_or_exceed_200_bytes",
                &[]
            )
        );
        ensure!(
            !self.prompt.trim().is_empty() && self.prompt.len() <= 64 * 1024,
            aow_i18n::message(
                "automations.task_content_must_not_be_empty_or_exceed_64",
                &[]
            )
        );
        self.workspace.validate()?;
        ensure!(
            self.workspace.workspace_mode != WorkspaceMode::Dynamic
                || self.kind == TaskKind::Manual,
            aow_i18n::message(
                "automations.only_manual_tasks_support_dynamic_workspaces",
                &[]
            )
        );
        ensure!(
            (1..=10).contains(&self.max_concurrent_runs),
            aow_i18n::message(
                "automations.maximum_concurrency_must_be_between_1_and_10",
                &[]
            )
        );
        self.validate_schedule()?;
        self.validate_bindings()?;
        Ok(())
    }

    pub fn validate_bindings(&self) -> Result<()> {
        ensure!(
            self.kind == TaskKind::Manual || self.prompt_bindings.is_empty(),
            aow_i18n::message("automations.only_manual_tasks_support_input_variables", &[])
        );
        let mut previous_end = 0;
        for binding in &self.prompt_bindings {
            ensure!(
                !binding.name.trim().is_empty()
                    && binding.start >= previous_end
                    && binding.start < binding.end
                    && self.prompt.get(binding.start..binding.end)
                        == Some(binding.placeholder.as_str()),
                aow_i18n::message(
                    "automations.variable_configuration_does_not_match_task_content_edit_and",
                    &[]
                )
            );
            previous_end = binding.end;
        }
        Ok(())
    }

    /// Resolve a per-run directory on an execution copy, leaving the template intact.
    pub fn resolve_workspace(&mut self, path: Option<PathBuf>) -> Result<()> {
        if self.workspace.workspace_mode != WorkspaceMode::Dynamic {
            ensure!(
                path.is_none(),
                aow_i18n::message(
                    "automations.only_dynamic_tasks_accept_a_workspace_directory_at_runtime",
                    &[]
                )
            );
            return Ok(());
        }
        ensure!(
            self.kind == TaskKind::Manual,
            aow_i18n::message(
                "automations.only_manual_tasks_support_dynamic_workspaces",
                &[]
            )
        );
        let workspace = WorkspaceConfig {
            workspace_mode: WorkspaceMode::Existing,
            workspace_path: path
                .filter(|path| !path.as_os_str().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(aow_i18n::message(
                        "automations.specify_a_workspace_directory_for_this_run",
                        &[]
                    ))
                })?,
            base_branch: String::new(),
        };
        workspace.validate()?;
        self.workspace = workspace;
        Ok(())
    }

    pub fn render_prompt(&self, values: &BTreeMap<String, String>) -> Result<String> {
        self.validate_bindings()?;
        ensure!(
            values.keys().all(|name| self
                .prompt_bindings
                .iter()
                .any(|binding| &binding.name == name)),
            aow_i18n::message("automations.contains_unconfigured_variables", &[])
        );
        let mut prompt = String::new();
        let mut offset = 0;
        for binding in &self.prompt_bindings {
            let value = values
                .get(&binding.name)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(aow_i18n::message(
                        "automations.enter_a_value_for_variable",
                        &[("auto0", binding.name.to_string())]
                    ))
                })?;
            prompt.push_str(&self.prompt[offset..binding.start]);
            prompt.push_str(value);
            ensure!(
                prompt.len() <= 64 * 1024,
                aow_i18n::message(
                    "automations.expanded_task_content_must_not_exceed_64_kib",
                    &[]
                )
            );
            offset = binding.end;
        }
        prompt.push_str(&self.prompt[offset..]);
        ensure!(
            prompt.len() <= 64 * 1024,
            aow_i18n::message(
                "automations.expanded_task_content_must_not_exceed_64_kib",
                &[]
            )
        );
        Ok(prompt)
    }
}

#[derive(Serialize, Deserialize)]
pub struct ManualRunRequest {
    pub task: Task,
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub hosted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentLaunch {
    pub executable: PathBuf,
    pub args: Vec<String>,
    /// Agent configuration locations only; PATH is loaded from shared Settings per run.
    /// Legacy PATH snapshots are accepted on read but ignored during execution.
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(flatten)]
    pub input: TaskInput,
    pub project_name: String,
    pub repository_path: PathBuf,
    pub launch: AgentLaunch,
    pub scheduler_error: Option<String>,
    #[serde(default)]
    pub deleted: bool,
}
