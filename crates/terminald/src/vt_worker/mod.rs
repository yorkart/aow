//! Headless VT worker process integration and per-session synchronization.

mod command;
mod config;
mod queue;
mod rpc;
mod session;
mod snapshot;
mod transport;
mod worker;

pub use config::VtWorkerConfig;
pub(crate) use session::{VtSession, VtSnapshot, VtWorkerClient};
pub(crate) use worker::VtWorker;

#[cfg(test)]
mod tests;
