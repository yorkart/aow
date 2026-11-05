//! CLI-created interactive terminals. Initialization and submission own an
//! exclusive lease; browsers may observe, or explicitly acquire a read lease
//! after initialization before claiming terminal input.

pub(in crate::terminal) mod api;
pub(in crate::terminal) mod connection;
mod lifecycle;
mod model;

#[cfg(test)]
use super::*;
const ROWS: u16 = 48;
const COLS: u16 = 160;

#[cfg(test)]
use aow_api_contract::terminal::{
    AgentTerminalCreate, AgentTerminalInfo, AgentTerminalPhase, AgentTerminalState,
    AgentTerminalSubmit,
};
#[cfg(test)]
use aow_host_contract::terminal::{TerminalAttachServerMessage, TerminalControlState};
#[cfg(test)]
use aow_operation_log::Outcome;
pub(crate) use api::{create, get, list, submit};
#[cfg(test)]
use connection::AgentConnection;
#[cfg(test)]
use tokio_tungstenite::tungstenite;

#[cfg(test)]
mod tests;
