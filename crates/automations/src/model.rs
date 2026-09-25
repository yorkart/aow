use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use aow_agents::automation::AutomationAgent as AgentKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    Existing,
    NewWorktree,
    NewBranch,
    Temporary,
}

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
    pub project_id: String,
    pub workspace_mode: WorkspaceMode,
    pub workspace_path: PathBuf,
    /// Legacy presentation field retained for saved task compatibility.
    ///
    /// Per-run Worktrees are always removed after execution; the runner does
    /// not use this value to decide whether cleanup occurs.
    #[serde(default = "default_cleanup_worktree")]
    pub cleanup_worktree: bool,
    #[serde(default)]
    pub base_branch: String,
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
    #[serde(default)]
    pub precheck_command: String,
    #[serde(default = "default_precheck_timeout")]
    pub precheck_timeout_seconds: u64,
    /// Delivery preference only. The server handles notifications independently.
    #[serde(default)]
    pub failure_notification: Option<FailureNotification>,
}

fn default_precheck_timeout() -> u64 {
    60
}

fn default_cleanup_worktree() -> bool {
    true
}

fn default_yolo() -> bool {
    true
}

impl TaskInput {
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
        ensure!(self.workspace_path.is_absolute(), "工作区必须使用绝对路径");
        ensure!(
            !self
                .workspace_path
                .to_string_lossy()
                .chars()
                .any(char::is_control),
            "工作区路径包含控制字符"
        );
        ensure!(self.precheck_command.len() <= 8192, "执行前检查命令过长");
        ensure!(
            (1..=3600).contains(&self.precheck_timeout_seconds),
            "检查超时必须为 1–3600 秒"
        );
        ensure!(
            (1..=10).contains(&self.max_concurrent_runs),
            "最大同时执行数必须为 1–10"
        );
        if matches!(
            self.workspace_mode,
            WorkspaceMode::NewWorktree | WorkspaceMode::NewBranch
        ) {
            ensure!(
                !self.base_branch.trim().is_empty()
                    && !self.base_branch.starts_with('-')
                    && !self.base_branch.chars().any(char::is_control),
                "请选择有效的基础分支"
            );
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    Scheduled,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Preparing,
    Running,
    Completed,
    Failed,
    Skipped,
    Interrupted,
}

impl RunStatus {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Preparing | Self::Running)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutput {
    Stdio,
    Stderr,
}

impl RunOutput {
    pub const ALL: [Self; 2] = [Self::Stdio, Self::Stderr];

    pub fn filename(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Stderr => "stderr",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub task_revision: u64,
    pub task_name: String,
    pub agent: AgentKind,
    pub source: RunSource,
    /// Input values captured for this manual invocation. None means the
    /// historical record did not capture parameters; an empty map means none.
    #[serde(default)]
    pub variables: Option<BTreeMap<String, String>>,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub workspace_path: Option<PathBuf>,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub agent_pid: Option<u32>,
    /// Exact executable and argv passed to the Agent process. Environment and
    /// stdin prompt content are deliberately excluded from execution history.
    #[serde(default)]
    pub agent_command: Option<Vec<String>>,
    pub exit_code: Option<i32>,
    pub message: Option<String>,
    pub preparation_ms: Option<u64>,
    pub session_acquired_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        run: Box<Run>,
        configuration: TaskInput,
    },
    Workspace {
        path: PathBuf,
        #[serde(default)]
        branch: Option<String>,
        elapsed_ms: u64,
    },
    AgentStarted {
        pid: u32,
        #[serde(default)]
        command: Vec<String>,
    },
    Session {
        session_id: String,
        elapsed_ms: u64,
    },
    Finished {
        at: DateTime<Utc>,
        status: RunStatus,
        exit_code: Option<i32>,
        message: Option<String>,
        duration_ms: u64,
    },
    OutputTruncated {
        output: RunOutput,
        limit_bytes: u64,
    },
}
