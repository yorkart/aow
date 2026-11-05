//! ACP adapter execution component embedded by the Host node daemon.
mod host;
mod runtime;
mod wire;
pub use runtime::{AcpProber, AcpRuntime, NodeOwnership};

mod conversations;
