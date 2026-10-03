//! One binding per terminal instance, fixed candidates per registration, and
//! a shared budget of forward-only transcript readers.

#[cfg(test)]
use super::*;
use std::time::Duration;

mod api;
mod manager;
mod model;
mod registry;
mod sources;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);

pub(super) use api::events;
pub(crate) use model::{TaskStopNotification, TaskStopSource};

impl super::TerminalManager {
    /// Internal consumers receive the same native events regardless of UI/IM preferences.
    pub(in crate::terminal) fn subscribe_task_completions(
        &self,
    ) -> tokio::sync::broadcast::Receiver<TaskStopNotification> {
        self.inner.task_completions.subscribe()
    }

    pub(crate) fn subscribe_task_stops(
        &self,
    ) -> tokio::sync::broadcast::Receiver<TaskStopNotification> {
        self.inner.task_stops.subscribe()
    }
}
#[cfg(test)]
use sources::add_sources;

#[cfg(test)]
mod tests;
