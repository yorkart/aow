use serde::{Deserialize, Serialize};

use super::runtime::TerminalRuntimeStatus;

/// Text messages accepted by the runtime attach WebSocket. Binary messages are
/// terminal input bytes and therefore are intentionally not represented here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalAttachClientMessage {
    Observe,
    Claim {
        force: bool,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Acknowledged PTY input for programmatic clients.
    Write {
        request_id: String,
        data: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalControlState {
    Claimed,
    /// The attachment receives terminal output but cannot write input or
    /// change the PTY geometry while another attachment controls it.
    Observing,
    Waiting,
}

/// Text messages produced by the runtime attach WebSocket. Terminal output and
/// scrollback are sent as binary WebSocket messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalAttachServerMessage {
    Written {
        request_id: String,
    },
    Control {
        state: TerminalControlState,
    },
    Stream {
        epoch: String,
        offset: u64,
        reset: bool,
        replay_bytes: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore: Option<String>,
        /// Original width of a serialized VT snapshot carried in `restore`.
        ///
        /// These dimensions are absent for the legacy mode-only repair
        /// sequence. When present, both dimensions must be present and the
        /// receiver must resize its emulator before applying `restore`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_cols: Option<u16>,
        /// Original height of a serialized VT snapshot carried in `restore`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_rows: Option<u16>,
    },
    Resized {
        cols: u16,
        rows: u16,
    },
    Status {
        status: TerminalRuntimeStatus,
        exit_code: Option<u32>,
    },
    Error {
        code: String,
        message: String,
    },
}
