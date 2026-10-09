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
    pub agent_profile_id: Option<String>,
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
    pub fn agent_profile_id(&self) -> &str {
        self.agent_profile_id
            .as_deref()
            .unwrap_or_else(|| self.agent.id())
    }

    pub fn validate_schedule(&self) -> Result<()> {
        if self.kind == TaskKind::Manual {
            ensure!(
                self.cron.is_empty() && self.interval_seconds.is_none(),
                "手动任务无需运行计划"
            );
            return Ok(());
        }
        if let Some(seconds) = self.interval_seconds {
            ensure!(
                (1..=2_678_400).contains(&seconds),
                "运行间隔必须为 1 秒至 31 天"
            );
        } else {
            crate::Schedule::parse(&self.cron)?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 200,
            "名称不能为空且不能超过 200 字节"
        );
        ensure!(
            !self.prompt.trim().is_empty() && self.prompt.len() <= 64 * 1024,
            "任务内容不能为空且不能超过 64 KiB"
        );
        self.workspace.validate()?;
        ensure!(
            self.workspace.workspace_mode != WorkspaceMode::Dynamic
                || self.kind == TaskKind::Manual,
            "仅手动任务支持动态指定工作区"
        );
        ensure!(
            (1..=10).contains(&self.max_concurrent_runs),
            "最大同时执行数必须为 1–10"
        );
        self.validate_schedule()?;
        self.validate_bindings()?;
        Ok(())
    }

    pub fn validate_bindings(&self) -> Result<()> {
        ensure!(
            self.kind == TaskKind::Manual || self.prompt_bindings.is_empty(),
            "仅手动任务支持输入变量"
        );
        let mut previous_end = 0;
        for binding in &self.prompt_bindings {
            ensure!(
                !binding.name.trim().is_empty()
                    && binding.start >= previous_end
                    && binding.start < binding.end
                    && self.prompt.get(binding.start..binding.end)
                        == Some(binding.placeholder.as_str()),
                "变量配置与任务内容不一致，请编辑任务后重新保存"
            );
            previous_end = binding.end;
        }
        Ok(())
    }

    /// Resolve a per-run directory on an execution copy, leaving the template intact.
    pub fn resolve_workspace(&mut self, path: Option<PathBuf>) -> Result<()> {
        if self.workspace.workspace_mode != WorkspaceMode::Dynamic {
            ensure!(path.is_none(), "仅动态指定的任务支持执行时传入工作区目录");
            return Ok(());
        }
        ensure!(
            self.kind == TaskKind::Manual,
            "仅手动任务支持动态指定工作区"
        );
        let workspace = WorkspaceConfig {
            workspace_mode: WorkspaceMode::Existing,
            workspace_path: path
                .filter(|path| !path.as_os_str().is_empty())
                .ok_or_else(|| anyhow::anyhow!("请在执行时指定工作区目录"))?,
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
            "包含未配置的变量"
        );
        let mut prompt = String::new();
        let mut offset = 0;
        for binding in &self.prompt_bindings {
            let value = values
                .get(&binding.name)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("请填写变量：{}", binding.name))?;
            prompt.push_str(&self.prompt[offset..binding.start]);
            prompt.push_str(value);
            ensure!(prompt.len() <= 64 * 1024, "替换后的任务内容不能超过 64 KiB");
            offset = binding.end;
        }
        prompt.push_str(&self.prompt[offset..]);
        ensure!(prompt.len() <= 64 * 1024, "替换后的任务内容不能超过 64 KiB");
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
