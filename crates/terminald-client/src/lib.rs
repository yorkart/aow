mod attach;
mod client;
mod error;
mod paths;
mod response;
mod socket;
mod transport;

pub use aow_protocol::{
    ApiError, TerminalAgentList, TerminalAttachClientMessage, TerminalAttachServerMessage,
    TerminalControlState, TerminalRuntime, TerminalRuntimeList, TerminalRuntimeSpec,
    TerminalRuntimeStatus, TerminaldHealth,
};
pub use attach::TerminaldAttachStream;
pub use client::TerminaldClient;
pub use error::TerminaldClientError;
pub use socket::default_socket_path;

#[cfg(test)]
mod tests;
