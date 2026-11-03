//! Same-user CLI control API client, separate from runtime node networking.
mod client;
mod error;
mod response;
mod transport;
pub use client::LocalClient;
pub use error::LocalClientError;
