//! Runtime-owned adapter installation and local ACP execution.
mod adapter;
mod host;
mod installation;
mod manager;
mod node_runtime;
mod project;
mod service;
mod transport;

pub use acp_client::{
    AcpHost, AgentInfo, ConnectionInfo, ConnectionStatus, Content, ExecutionEnvironment,
    Permission, PromptReceipt, SETTINGS_PATH, SessionCreation, SessionImport, SessionInfo,
    SessionSnapshot, SessionSource, SessionUpdates, SessionVersion, SettingsDocument, SyncEvent,
    SyncResource, ThreadEntry, TurnCompletion, is_auth_required, validate_settings,
};
pub use adapter::AdapterCommand;
pub use service::AcpService;
