//! In-memory PTY runtime daemon served over a local Unix socket.
//!
//! `terminald` owns processes only. It deliberately has no tab, layout, or
//! persistence layer; callers own those durable concepts and address a
//! runtime by an opaque ID.

mod error;
mod runtime;
mod vt_worker;

pub use error::TerminaldError;
pub use runtime::{
    build_router, default_socket_path, run, run_with_shutdown, run_with_shutdown_and_vt_worker,
};
pub use vt_worker::VtWorkerConfig;
