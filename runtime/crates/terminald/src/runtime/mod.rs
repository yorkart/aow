//! PTY lifecycle, runtime ownership, scrollback, and process cleanup.

use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use aow_protocol::{
    TerminalAgentList, TerminalAttachClientMessage, TerminalAttachServerMessage,
    TerminalControlState, TerminalPaneStatus, TerminalRuntime, TerminalRuntimeList,
    TerminalRuntimeSpec, TerminaldHealth,
};
use axum::{
    Json, Router,
    extract::{
        Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use bytes::Bytes;
use futures_util::{Sink, SinkExt, StreamExt};
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Deserialize;
use tokio::{
    sync::{Mutex as AsyncMutex, Notify, broadcast, watch},
    time::timeout,
};

const SERVICE_NAME: &str = "aow-terminald";
const SCROLLBACK_LIMIT: usize = 8 * 1024 * 1024;
const REPLAY_CHUNK_SIZE: usize = 64 * 1024;
const OUTPUT_CHANNEL_CAPACITY: usize = 512;
const SOCKET_SEND_TIMEOUT: Duration = Duration::from_secs(2);
const SOCKET_WRITE_BUFFER_SIZE: usize = 64 * 1024;
// An 8 MiB serialized ANSI snapshot can expand up to 6x when control
// bytes are escaped inside its JSON text frame. Keep the WebSocket buffer
// bounded while leaving enough room for that single restore message.
const SOCKET_MAX_WRITE_BUFFER_SIZE: usize = 64 * 1024 * 1024;
const MAX_RUNTIME_ID_BYTES: usize = 1024;
const DELETE_REAP_TIMEOUT: Duration = Duration::from_secs(10);

use crate::{
    TerminaldError,
    vt_worker::{VtSession, VtSnapshot, VtWorker, VtWorkerClient},
};

mod agents;
mod embedded;
mod http;
mod output;
mod process;
#[expect(
    clippy::module_inception,
    reason = "Keep the runtime implementation beside its focused sibling modules"
)]
mod runtime;
mod session_titles;
mod state;
mod validation;
mod websocket;

use output::*;
use process::*;
use runtime::*;
use state::*;
use validation::*;

pub use embedded::{TerminalObserver, TerminalRuntimeHost};
pub use http::build_router;

#[cfg(test)]
mod tests;
