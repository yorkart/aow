//! PTY lifecycle, runtime ownership, scrollback, and process cleanup.

use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
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
use hyper::server::conn::http1;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Deserialize;
use tokio::{
    net::UnixListener,
    sync::{Mutex as AsyncMutex, Notify, broadcast, watch},
    task::JoinSet,
    time::timeout,
};

const SERVICE_NAME: &str = "aow-terminald";
const SCROLLBACK_LIMIT: usize = 8 * 1024 * 1024;
const REPLAY_CHUNK_SIZE: usize = 64 * 1024;
const OUTPUT_CHANNEL_CAPACITY: usize = 512;
const SOCKET_SEND_TIMEOUT: Duration = Duration::from_secs(2);
const SOCKET_WRITE_BUFFER_SIZE: usize = 64 * 1024;
// A 2 MiB serialized ANSI snapshot can expand substantially when control
// bytes are escaped inside its JSON text frame. Keep the WebSocket buffer
// bounded while leaving enough room for that single restore message.
const SOCKET_MAX_WRITE_BUFFER_SIZE: usize = 16 * 1024 * 1024;
const MAX_RUNTIME_ID_BYTES: usize = 1024;
const DELETE_REAP_TIMEOUT: Duration = Duration::from_secs(10);

use crate::{
    TerminaldError,
    vt_worker::{VtSession, VtSnapshot, VtWorker, VtWorkerClient},
};

mod agents;
mod http;
mod output;
mod process;
mod runtime;
mod server;
mod socket;
mod state;
mod validation;
mod websocket;

use output::*;
use process::*;
use runtime::*;
use state::*;
use validation::*;

pub use http::build_router;
use http::router_with_state;
pub use server::{run, run_with_shutdown, run_with_shutdown_and_vt_worker};
pub use socket::default_socket_path;

#[cfg(test)]
mod tests;
