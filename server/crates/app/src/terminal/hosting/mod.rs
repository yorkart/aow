//! Server-owned terminal control and durable automation rounds.

mod api;
mod source;
mod state;
mod worker;

pub(super) use api::enable;
pub(crate) use worker::recover;

use super::*;
use aow_api_contract::terminal::{TerminalHosting, TerminalHostingPhase};

#[cfg(test)]
mod tests;
