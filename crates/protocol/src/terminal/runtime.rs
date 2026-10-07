use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::model::TerminalPaneStatus;

/// Text from the current VT viewport, never an accumulation of raw redraws.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalScreen {
    pub generation: String,
    pub applied_offset: u64,
    pub cols: u16,
    pub rows: u16,
    pub lines: Vec<String>,
}

/// Identity and version information returned by a terminald instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminaldHealth {
    pub service: String,
    pub instance_id: String,
    pub version: String,
    /// Used for process ownership checks and launchd activation.
    /// Optional for compatibility with older daemons that omit their PID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

/// The immutable process specification used for idempotent runtime creation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalRuntimeSpec {
    pub cwd: String,
    pub shell: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    pub rows: u16,
    pub cols: u16,
}

/// Runtime status uses the same wire values as the browser terminal protocol.
pub type TerminalRuntimeStatus = TerminalPaneStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalRuntime {
    pub id: String,
    pub cwd: String,
    pub shell: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    /// Launch-only values are retained inside terminald for idempotency but
    /// are never serialized back to HTTP clients.
    #[serde(skip)]
    pub environment: BTreeMap<String, String>,
    pub status: TerminalRuntimeStatus,
    pub rows: u16,
    pub cols: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
}

impl TerminalRuntime {
    /// Returns the runtime's currently reported process fields and PTY size.
    ///
    /// The daemon compares idempotent PUT requests against its immutable
    /// creation spec internally; after a WebSocket resize, this value reflects
    /// the live size and is therefore not necessarily that creation spec.
    pub fn spec(&self) -> TerminalRuntimeSpec {
        TerminalRuntimeSpec {
            cwd: self.cwd.clone(),
            shell: self.shell.clone(),
            arguments: self.arguments.clone(),
            environment: self.environment.clone(),
            rows: self.rows,
            cols: self.cols,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalRuntimeList {
    pub runtimes: Vec<TerminalRuntime>,
}
