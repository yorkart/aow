//! HTTP management and WebSocket transport for node-local terminals.
//!
//! The embedding daemon owns the listener, authentication and terminal host.

mod error;
mod http;
mod websocket;

pub use http::router;
