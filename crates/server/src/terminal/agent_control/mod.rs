//! CLI-created interactive terminals. Initialization and submission own an
//! exclusive lease; browsers may observe, or explicitly acquire a read lease
//! after initialization before claiming terminal input.

mod api;
mod connection;
mod lifecycle;
mod model;

#[cfg(test)]
use super::*;
const ROWS: u16 = 48;
const COLS: u16 = 160;

#[cfg(test)]
use aow_agents::Agent;
#[cfg(test)]
use aow_operation_log::Outcome;
#[cfg(test)]
use aow_protocol::{
    AgentTerminalCreate, AgentTerminalInfo, AgentTerminalPhase, AgentTerminalState,
    AgentTerminalSubmit, TerminalAttachServerMessage, TerminalControlState,
};
pub(crate) use api::{create, get, list, submit};
#[cfg(test)]
use connection::AgentConnection;
#[cfg(test)]
use tokio_tungstenite::tungstenite;

#[cfg(test)]
mod tests;
