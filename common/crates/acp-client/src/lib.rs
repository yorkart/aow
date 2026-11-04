//! AoW ACP integration. Only the facade is a supported AoW dependency.

mod acp_thread;
mod agent_servers;
mod agent_ui;
mod facade;
mod settings;

pub use facade::{
    AcpConnector, AcpHost, AcpService, AdapterTransport, AgentInfo, ConnectionInfo,
    ConnectionStatus, Content, ExecutionEnvironment, Permission, PromptReceipt, SessionCreation,
    SessionImport, SessionInfo, SessionSnapshot, SessionSource, SessionUpdates, SessionVersion,
    SyncEvent, SyncResource, ThreadEntry, TurnCompletion, is_auth_required,
};
pub use settings::{SETTINGS_PATH, SettingsDocument, validate_settings};

pub use agent_servers::progress::Progress as OperationProgress;
pub use facade::events::SyncEvents;
pub use settings::{AgentConfigOptionValue, CustomAgentServerSettings};
