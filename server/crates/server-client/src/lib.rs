//! Server business API client over authenticated HTTP or a same-user Unix socket, separate from Host node networking.
mod client;
mod error;
mod response;
mod transport;
pub use client::ServerClient;
pub use error::ServerClientError;
