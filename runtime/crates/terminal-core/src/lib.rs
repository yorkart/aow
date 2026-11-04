//! Node-local terminal execution, PTY ownership and transport-independent attachments.
//!
//! Callers own durable tabs and layouts and address terminals by opaque IDs.
//! HTTP and WebSocket integration lives in `aow-terminal-service`.

mod error;
mod node_worker;
mod runtime;
mod vt_worker;

pub use error::TerminalError;
pub use node_worker::configure as configure_vt_worker;
pub use runtime::{
    AttachOptions, CreateOutcome, TerminalConnection, TerminalEvent, TerminalInput,
    TerminalManager, TerminalObserver, TerminalRuntimeHost,
};
pub use vt_worker::VtWorkerConfig;
