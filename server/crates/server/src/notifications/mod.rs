//! Local notification settings and dispatch, independent of agent implementations.

mod api;
mod manager;
mod messages;
mod security;
mod settings;

pub(crate) use api::routes;
pub(crate) use manager::NotificationManager;
pub(crate) use settings::{AutomationFailureNotification, Channel, SettingsUpdate, SettingsView};

#[cfg(test)]
use crate::AppState;
#[cfg(test)]
use crate::terminal::notifications::TaskStopNotification;
#[cfg(test)]
use aow_im::{ImConfigUpdate, ImKind, Provider};
#[cfg(test)]
use axum::http::{StatusCode, header::CACHE_CONTROL};
use manager::QUEUE_CAPACITY;
use settings::Document;
#[cfg(test)]
use settings::{FILE_NAME, TaskCompletedSettings};
#[cfg(test)]
use std::{path::PathBuf, sync::Arc};
#[cfg(test)]
use tokio::sync::broadcast;

#[cfg(test)]
mod tests;
