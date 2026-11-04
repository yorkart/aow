use serde::{Deserialize, Serialize};

use super::runtime::TerminalRuntimeStatus;

/// Lossless restore negotiated with `snapshot_compression=gzip`. The data is
/// base64 gzip; decoded bytes are streamed into xterm before replay or input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalRestoreGzip {
    pub data: String,
    pub decoded_bytes: u64,
}

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_gzip: Option<TerminalRestoreGzip>,
        /// Original width of the snapshot in `restore` or `restore_gzip`.
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
