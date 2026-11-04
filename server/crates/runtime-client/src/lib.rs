//! HTTP/WebSocket SDK for local or remote runtime nodes.
pub mod acp;
mod client;
mod completions;
mod config;
pub use completions::{CompletionCursor, CompletionFrame};
mod environments;
mod executions;
pub use executions::{ExecutionEvent, ExecutionReference, ExecutionSnapshot};
mod error;
pub use environments::{EnvironmentPreparation, NodeEnvironment};
mod management;
mod workspace;
pub use workspace::{GitCommandResult, NodeMetadata, WorkspaceChanges};
mod sessions;
mod terminal;

pub use aow_protocol::{
    ApiError, TerminalAgentList, TerminalAttachClientMessage, TerminalAttachServerMessage,
    TerminalControlState, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec,
    TerminalRuntimeStatus, TerminaldHealth,
};
pub use client::{RuntimeClient, RuntimeSocket};
pub use config::RuntimeEndpoint;
pub use error::{RuntimeError, is_unavailable};
pub use terminal::{TerminalAttachOptions, TerminalClient};

#[cfg(feature = "test-support")]
pub mod testing;

pub use management::{AcpInstance, AcpInstanceSpec, AcpInstanceStatus};

pub use sessions::{NativePaneSessions, NativeSession, NativeSessionReference};
