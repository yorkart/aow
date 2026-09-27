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
use uuid::Uuid;

use crate::{
    DELETE_REAP_TIMEOUT, MAX_RUNTIME_ID_BYTES, OUTPUT_CHANNEL_CAPACITY, REPLAY_CHUNK_SIZE,
    SCROLLBACK_LIMIT, SERVICE_NAME, SOCKET_MAX_WRITE_BUFFER_SIZE, SOCKET_SEND_TIMEOUT,
    SOCKET_WRITE_BUFFER_SIZE, TerminaldError,
    vt_worker::{VtSession, VtSnapshot, VtWorker, VtWorkerClient},
};

mod agents;
mod http;
#[path = "runtime.rs"]
mod implementation;
mod output;
mod process;
mod server;
mod state;
mod validation;
mod websocket;

use implementation::*;
use output::*;
use process::*;
use state::*;
use validation::*;

pub(crate) use http::build_router;
use http::router_with_state;
pub use server::{default_socket_path, run, run_with_shutdown, run_with_shutdown_and_vt_worker};

#[cfg(test)]
mod tests;
