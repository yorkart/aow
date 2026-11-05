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

use aow_host_contract::terminal::{
    TerminalAgentList, TerminalAttachClientMessage, TerminalAttachServerMessage,
    TerminalControlState, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec,
    TerminalRuntimeStatus, TerminaldHealth,
};
use bytes::Bytes;
use futures_util::{Sink, SinkExt, StreamExt};
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tokio::{
    sync::{Mutex as AsyncMutex, Notify, broadcast, watch},
    time::timeout,
};

const SERVICE_NAME: &str = "aow-terminald";
const SCROLLBACK_LIMIT: usize = 8 * 1024 * 1024;
const REPLAY_CHUNK_SIZE: usize = 64 * 1024;
const OUTPUT_CHANNEL_CAPACITY: usize = 512;
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RUNTIME_ID_BYTES: usize = 1024;
const DELETE_REAP_TIMEOUT: Duration = Duration::from_secs(10);

use crate::{
    TerminalError,
    vt_worker::{VtSession, VtSnapshot, VtWorker, VtWorkerClient},
};

mod agent_operations;
mod agents;
mod api;
mod attachment;
mod embedded;
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

use output::*;
use process::*;
use runtime::*;
use state::*;
use validation::*;

pub use api::{AttachOptions, TerminalConnection, TerminalManager};
pub use attachment::{TerminalEvent, TerminalInput};
pub use embedded::{TerminalObserver, TerminalRuntimeHost};
pub use state::CreateOutcome;

#[cfg(test)]
mod tests;
