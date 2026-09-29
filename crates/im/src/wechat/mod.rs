//! Tencent iLink transport, with Hermes-style one-shot sends and context fallback.
//! No dependency on a browser, desktop WeChat, OpenClaw or the AoW server.

mod api;
mod client;
mod login;
mod model;
mod transport;

#[cfg(test)]
use crate::{ImProvider, Message};
#[cfg(test)]
use serde_json::json;
#[cfg(test)]
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
#[cfg(test)]
use tokio::task::JoinHandle;

pub use login::{LoginManager, LoginView};
pub use model::{ConnectionStatus, Credentials, TestReceipt, TestVerification, WechatClient};

#[cfg(test)]
mod tests;
