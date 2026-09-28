mod configuration;
mod delivery;
mod task;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use aow_im::{ImConfig, Provider};
use tokio::sync::mpsc;

use super::{Document, security};
use crate::terminal::notifications::TaskStopNotification;

pub(super) const QUEUE_CAPACITY: usize = 64;

pub(super) struct ConfiguredProvider {
    pub(super) config: ImConfig,
    pub(super) provider: Provider,
}

pub(super) struct RuntimeState {
    pub(super) document: Document,
    pub(super) providers: Vec<ConfiguredProvider>,
    pub(super) revision: u64,
}

pub(super) struct Delivery {
    pub(super) event: TaskStopNotification,
    pub(super) revision: u64,
    pub(super) id: String,
}

pub(super) struct Inner {
    pub(super) security: security::Dispatcher,
    path: Option<PathBuf>,
    started: AtomicBool,
    wechat_login: aow_im::wechat::LoginManager,
    pub(super) state: Mutex<RuntimeState>,
    sender: mpsc::Sender<Delivery>,
    pub(super) receiver: Mutex<Option<mpsc::Receiver<Delivery>>>,
}

#[derive(Clone)]
pub(crate) struct NotificationManager {
    pub(super) inner: Arc<Inner>,
}
