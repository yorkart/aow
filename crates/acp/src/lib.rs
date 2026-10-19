//! AoW ACP integration. Only the facade is a supported AoW dependency.

mod acp_thread;
mod agent_servers;
mod agent_ui;
mod facade;
mod node_runtime;
mod project;
mod settings;

pub use facade::{
    AcpHost, AcpService, AgentInfo, ConnectionInfo, ConnectionStatus, Content, Permission,
    SessionCreation, SessionImport, SessionInfo, SessionSnapshot, SessionSource, SessionUpdates,
    SessionVersion, SyncEvent, SyncResource, ThreadEntry, TurnCompletion, is_auth_required,
};
pub use settings::{SETTINGS_PATH, SettingsDocument, validate_settings};
