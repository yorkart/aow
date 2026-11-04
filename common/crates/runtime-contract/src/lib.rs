//! Shared HTTP management and event contracts; independent of client and node implementations.
//! Native ACP conversation messages are handled separately by agent-client-protocol.
mod acp;
pub use acp::{AcpInstance, AcpInstanceSpec, AcpInstanceStatus, AcpSettings, AcpSettingsUpdate};

mod completion;
mod environment;
mod execution;
mod sessions;
mod workspace;
pub use completion::{CompletionCursor, CompletionFrame};
pub use environment::{EnvironmentPreparation, ExecutionEnvironment, NodeEnvironment};
pub use execution::{ExecutionEvent, ExecutionReference, ExecutionSnapshot};
pub use sessions::{NativePaneSessionRecord, NativeSessionRecord, NativeSessionReference};
pub use workspace::{GitCommandResult, NodeMetadata, WorkspaceChanges};
