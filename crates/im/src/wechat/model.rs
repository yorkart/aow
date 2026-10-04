use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use super::api::Api;

// No Debug: these values are local secrets, never part of the settings view.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub account_id: String,
    pub user_id: String,
    pub base_url: String,
    pub bot_token: String,
    pub binding_id: String,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Session {
    pub(super) cursor: String,
    pub(super) context_token: Option<String>,
    #[serde(default)]
    pub(super) verification: Option<TestVerification>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestVerification {
    pub test_id: String,
    pub receipt: TestReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestReceipt {
    Sent,
    Confirmed,
    Missing,
}

#[derive(Clone, Default, Serialize)]
pub struct ConnectionStatus {
    pub receiving: bool,
    pub context_ready: bool,
    pub error: Option<String>,
    pub verification: Option<TestVerification>,
}

pub(super) struct Inner {
    pub(super) active: AtomicBool,
    pub(super) api: Api,
    pub(super) credentials: Credentials,
    pub(super) path: Option<PathBuf>,
    pub(super) session: Mutex<Session>,
    pub(super) status: Mutex<ConnectionStatus>,
    pub(super) sending: tokio::sync::Mutex<()>,
}

pub struct WechatClient {
    pub(super) inner: Arc<Inner>,
    pub(super) receiver: Mutex<Option<JoinHandle<()>>>,
}
