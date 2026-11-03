//! Client for the same-user ACP daemon, with explicit embedding for library hosts.
mod client;
mod methods;
pub mod protocol;

pub use client::{AcpClient, default_socket_path, is_auth_required, is_unavailable};
pub use protocol::{Event, Health, RpcError};
