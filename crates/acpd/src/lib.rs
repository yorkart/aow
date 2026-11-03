//! Same-user ACP execution service, independent of the AoW web process.
mod dispatch;
mod host;
mod probe;
mod runtime;
mod socket;

pub use runtime::{Options, run_with_shutdown};
