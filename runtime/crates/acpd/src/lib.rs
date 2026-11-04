//! ACP adapter execution component embedded by the runtime node daemon.
mod host;
mod runtime;
mod wire;
pub use runtime::{AcpProber, AcpRuntime, NodeOwnership};
