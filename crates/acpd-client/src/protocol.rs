//! Versioned local daemon contract; independent of the agent's ACP wire protocol.
use aow_acp::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub protocol: u32,
    pub language: aow_i18n::Language,
    pub request: Request,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Request {
    RecoverWorkspaceRemovals {},
    Probe {
        agent_id: String,
        cwd: PathBuf,
        execution: ExecutionEnvironment,
    },
    SyncEvent {
        resource: SyncResource,
    },
    Agents {},
    Registry {
        refresh: bool,
    },
    InstallAgent {
        agent_id: String,
    },
    InstallationStatus {
        agent_id: String,
    },
    Connect {
        agent_id: String,
        cwd: PathBuf,
    },
    ConnectionStatus {
        agent_id: String,
        cwd: PathBuf,
    },
    ConnectionRequests {
        connection_id: String,
    },
    AnswerConnection {
        connection_id: String,
        request_id: String,
        response: Value,
    },
    Disconnect {
        connection_id: String,
    },
    Authenticate {
        connection_id: String,
        method_id: String,
    },
    RemoteSessions {
        connection_id: String,
        cursor: Option<String>,
    },
    ImportSessions {
        connection_id: String,
        sessions: Vec<SessionImport>,
    },
    NewSession {
        connection_id: String,
    },
    ForkSession {
        thread_id: String,
    },
    SelectAgentId {
        explicit: Option<String>,
    },
    PrepareSession {
        agent_id: String,
        cwd: PathBuf,
    },
    PrepareSessionWithOptions {
        agent_id: String,
        cwd: PathBuf,
        creation: SessionCreation,
    },
    SessionInfo {
        thread_id: String,
    },
    SessionConnectionInfo {
        thread_id: String,
    },
    ThreadForSession {
        connection_id: String,
        session_id: String,
    },
    LoadSession {
        connection_id: String,
        session_id: String,
    },
    Resume {
        thread_id: String,
    },
    Sessions {
        cwd: Option<String>,
    },
    Snapshot {
        thread_id: String,
    },
    Overview {
        thread_id: String,
    },
    OverviewUpdates {
        thread_id: String,
        generation: String,
        revision: u64,
    },
    Process {
        thread_id: String,
        turn_id: String,
        generation: Option<String>,
        revision: Option<u64>,
    },
    Updates {
        thread_id: String,
        generation: String,
        revision: u64,
    },
    Logs {
        connection_id: String,
    },
    SessionLogs {
        thread_id: String,
    },
    StartPrompt {
        thread_id: String,
        content: Vec<Content>,
    },
    StartPromptOnce {
        thread_id: String,
        request_id: String,
        content: Vec<Content>,
    },
    Cancel {
        thread_id: String,
    },
    StartSteering {
        thread_id: String,
        content: Vec<Content>,
    },
    DismissNotice {
        thread_id: String,
        notice_id: String,
    },
    ClearPlan {
        thread_id: String,
    },
    SetMode {
        thread_id: String,
        mode: String,
    },
    SetConfigOption {
        thread_id: String,
        option: String,
        value: Value,
    },
    Answer {
        thread_id: String,
        request: String,
        response: Value,
    },
    CloseSession {
        thread_id: String,
    },
    SetWorkspaceRemoving {
        cwd: String,
        removing: bool,
    },
    DeleteWorkspace {
        cwd: String,
    },
    DeleteSession {
        thread_id: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Health {
    pub service: String,
    pub protocol: u32,
    pub instance_id: String,
    pub pid: u32,
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    Sync(SyncEvent),
    Completed {
        id: String,
        completion: TurnCompletion,
    },
}
