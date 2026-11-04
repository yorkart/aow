//! Server-owned credentials for CLI business APIs. Never linked by runtime.
mod access;
pub use access::{Access, Grant};
