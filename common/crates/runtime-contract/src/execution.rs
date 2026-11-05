pub use crate::agents::AgentType as AgentKind;
pub use crate::workspace::WorkspaceMode;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentLaunch {
    pub executable: PathBuf,
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionInput {
    pub agent: AgentKind,
    pub prompt: String,
    #[serde(flatten)]
    pub workspace: crate::workspace::WorkspaceConfig,
    #[serde(default)]
    pub yolo: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapacityReservation {
    pub key: String,
    pub limit: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionSpec {
    pub id: String,
    pub repository_path: PathBuf,
    pub launch: AgentLaunch,
    pub input: ExecutionInput,
    #[serde(default)]
    pub agent_environment: BTreeMap<String, String>,
    #[serde(default)]
    pub capacity: Option<CapacityReservation>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Preparing,
    Running,
    Completed,
    Failed,
    Interrupted,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutput {
    Stdio,
    Stderr,
}
impl RunOutput {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Stderr => "stderr",
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Workspace {
        path: PathBuf,
        branch: Option<String>,
        elapsed_ms: u64,
    },
    AgentStarted {
        pid: u32,
        command: Vec<String>,
    },
    Session {
        session_id: String,
        elapsed_ms: u64,
    },
    Finished {
        at: chrono::DateTime<chrono::Utc>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub sequence: u64,
    pub event: RunEvent,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ExecutionResult {
    Completed { conclusion: String },
    Unavailable { error: String },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    pub id: String,
    pub node_id: String,
    pub head: u64,
    pub terminal: bool,
    pub events: Vec<ExecutionEvent>,
    pub result: Option<ExecutionResult>,
    pub native_session: Option<crate::agents::NativeSession>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitExecution {
    pub node_id: String,
    pub spec: ExecutionSpec,
}
