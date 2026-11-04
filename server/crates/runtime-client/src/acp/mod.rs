//! Native ACP SDK client over the runtime node WebSocket transport.
mod client;
mod connector;
#[cfg(feature = "test-support")]
mod fixture;
mod host;
mod methods;
pub use crate::is_unavailable;
pub use aow_acp_client::{
    AcpHost, AgentInfo, ConnectionInfo, ConnectionStatus, Content, ExecutionEnvironment,
    Permission, PromptReceipt, SETTINGS_PATH, SessionCreation, SessionImport, SessionInfo,
    SessionSnapshot, SessionSource, SessionUpdates, SessionVersion, SettingsDocument, SyncEvent,
    SyncResource, ThreadEntry, TurnCompletion, validate_settings,
};
pub use client::{AcpClient, AcpSettings};
pub fn is_auth_required(error: &anyhow::Error) -> bool {
    aow_acp_client::is_auth_required(error)
}
