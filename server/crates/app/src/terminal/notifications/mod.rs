//! One binding per terminal instance, fixed candidates per registration, and
//! a shared budget of forward-only transcript readers.

#[cfg(test)]
use super::*;

mod api;
mod model;
mod remote;
#[cfg(test)]
mod remote_tests;
mod sources;

pub(super) use api::events;
pub(crate) use model::{TaskStopNotification, TaskStopSource};

impl super::TerminalManager {
    pub(crate) fn task_stop_sender(&self) -> tokio::sync::broadcast::Sender<TaskStopNotification> {
        self.inner.task_stops.clone()
    }

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
