//! ACP execution library with native WebSocket transport and a local compatibility daemon.
mod dispatch;
mod host;
mod probe;
mod runtime;
mod socket;
mod wire;

pub use runtime::{AcpRuntime, Options, run_with_shutdown};
