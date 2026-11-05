use super::ExecutionEnvironment;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpInstanceSpec {
    pub agent_id: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub execution_environment: Option<ExecutionEnvironment>,
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcpInstanceStatus {
    pub running: bool,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcpInstance {
    pub id: String,
    pub agent_id: String,
    pub cwd: PathBuf,
    pub pid: u32,
    pub attached: bool,
    pub status: AcpInstanceStatus,
    pub websocket_path: String,
    pub execution_home: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcpSettings {
    pub path: String,
    pub content: String,
    pub revision: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcpSettingsUpdate {
    pub content: String,
    pub revision: String,
}
