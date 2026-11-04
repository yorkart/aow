//! Native ACP SDK client over the runtime node WebSocket transport.
mod client;
mod connector;
mod host;
mod methods;
pub use crate::is_unavailable;
pub use aow_acp::*;
pub use client::{AcpClient, AcpSettings};
pub fn is_auth_required(error: &anyhow::Error) -> bool {
    aow_acp::is_auth_required(error)
}
