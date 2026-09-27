//! In-memory PTY runtime daemon served over a local Unix socket.
//!
//! `terminald` owns processes only. It deliberately has no tab, layout, or
//! persistence layer; callers own those durable concepts and address a
//! runtime by an opaque ID.

mod error;
mod runtime;
mod vt_worker;

pub use error::TerminaldError;
pub use runtime::{default_socket_path, run, run_with_shutdown, run_with_shutdown_and_vt_worker};
pub use vt_worker::VtWorkerConfig;

const SERVICE_NAME: &str = "aow-terminald";
const SCROLLBACK_LIMIT: usize = 8 * 1024 * 1024;
const REPLAY_CHUNK_SIZE: usize = 64 * 1024;
const OUTPUT_CHANNEL_CAPACITY: usize = 512;
const SOCKET_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const SOCKET_WRITE_BUFFER_SIZE: usize = 64 * 1024;
// A 2 MiB serialized ANSI snapshot can expand substantially when control
// bytes are escaped inside its JSON text frame. Keep the WebSocket buffer
// bounded while leaving enough room for that single restore message.
const SOCKET_MAX_WRITE_BUFFER_SIZE: usize = 16 * 1024 * 1024;
const MAX_RUNTIME_ID_BYTES: usize = 1024;
const DELETE_REAP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Build a router backed by a fresh, non-persistent terminald instance.
pub fn build_router() -> axum::Router {
    runtime::build_router()
}
