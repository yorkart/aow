use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use aow_protocol::{
    TerminalAgentList, TerminalAttachServerMessage, TerminalLayout, TerminalPane, TerminalPaneKind,
    TerminalPaneStatus, TerminalRuntime, TerminalRuntimeSpec, TerminalSplitAxis, TerminalTab,
    TerminalTabList,
};
use aow_terminald_client::{TerminaldAttachStream, TerminaldClient, TerminaldClientError};
#[cfg(test)]
use axum::Router;
use axum::{
    Json,
    body::Body,
    extract::{
        Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{
        HeaderMap, StatusCode, Uri,
        header::{CONTENT_LENGTH, HOST, ORIGIN},
    },
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite;
#[cfg(test)]
use uuid::Uuid;

use crate::{AppState, HttpError, aow::AgentLaunch};

pub(crate) mod agent_control;
mod api;
mod bridge;
mod clipboard;
mod error;
pub(crate) mod hosting;
mod layout;
mod lifecycle;
pub(crate) mod notifications;
mod panes;
mod persistence;
mod rebuild;
mod requests;
mod runtime;
mod session_titles;
mod sessions;
mod state;
mod tabs;
mod validation;

pub(crate) use api::routes;
#[cfg(test)]
pub(crate) use api::validate_request_origin;
pub use error::TerminalError;
pub(crate) use error::terminal_http_error;
use layout::{
    remove_layout_leaf, replace_layout_leaf, tab_index, touch_tab, validate_layout,
    validate_persisted_tabs, validate_ratio,
};
use panes::find_pane;
use persistence::{atomic_save, timestamp};
use requests::*;
use runtime::{
    apply_runtime, client_error_is_not_found, map_client_error, map_create_error, runtime_spec,
};
pub(crate) use state::TerminalManager;
use state::{ManagerInner, ManagerState, PersistedState};
use validation::{
    default_pane_name, migrate_terminal_names, normalize_shell, resolve_cwd, terminal_size,
    validate_directory, validate_name,
};

const METADATA_FILE: &str = "terminals.json";
const METADATA_VERSION: u32 = 1;
const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const MAX_PANES_PER_TAB: usize = 64;
const MAX_LAYOUT_DEPTH: usize = 64;
const CLIPBOARD_DIRECTORY: &str = "clipboard-images";
const MAX_CLIPBOARD_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_CLIPBOARD_DIRECTORY_BYTES: u64 = 256 * 1024 * 1024;
const CLIPBOARD_IMAGE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const CLIPBOARD_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const BRIDGE_CLOSE_FORWARD_TIMEOUT: Duration = Duration::from_secs(1);
const BRIDGE_MESSAGE_FORWARD_TIMEOUT: Duration = Duration::from_secs(5);
const BRIDGE_RUNTIME_SYNC_TIMEOUT: Duration = Duration::from_secs(2);

pub use aow_filesystem::default_state_dir;

#[cfg(test)]
mod tests;
