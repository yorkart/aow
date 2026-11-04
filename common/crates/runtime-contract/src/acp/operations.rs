use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceRequest {
    pub operation: Operation,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub callback_environment: Option<BTreeMap<String, String>>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceError {
    pub code: String,
    pub message: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    SyncEvent {
        resource: SyncResource,
    },
    Agents,
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
    Disconnect {
        connection_id: String,
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
    DismissNotice {
        thread_id: String,
        notice_id: String,
    },
    ClearPlan {
        thread_id: String,
    },
    CloseSession {
        thread_id: String,
    },
    SetWorkspaceRemoving {
        cwd: String,
        removing: bool,
    },
    RecoverWorkspaceRemovals,
    DeleteWorkspace {
        cwd: String,
    },
    DeleteSession {
        thread_id: String,
    },
    Probe {
        agent_id: String,
        cwd: PathBuf,
        execution: ExecutionEnvironment,
    },
    Checkpoint,
}
