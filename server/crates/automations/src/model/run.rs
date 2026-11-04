use std::{collections::BTreeMap, path::PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AgentKind, task::TaskInput};

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
