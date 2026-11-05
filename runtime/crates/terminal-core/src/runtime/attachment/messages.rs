use aow_runtime_contract::terminal::{TerminalAttachClientMessage, TerminalAttachServerMessage};
use bytes::Bytes;

/// Typed terminal input, independent of network framing or serialization.
#[derive(Debug)]
pub enum TerminalInput {
    Data(Bytes),
    Command(TerminalAttachClientMessage),
    /// A caller could not decode a command; report it without ending the attachment.
    InvalidCommand(String),
}

/// Terminal output delivered in order with controller changes and replay boundaries.
#[derive(Debug)]
pub enum TerminalEvent {
    Output(Bytes),
    Message(TerminalAttachServerMessage),
}
