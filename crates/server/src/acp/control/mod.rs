//! AoW view ownership. This never changes an ACP adapter's session lifecycle.
#[cfg(test)]
mod http_tests;
mod socket;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod websocket_tests;

use crate::HttpError;
use axum::http::{HeaderMap, StatusCode};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard, watch};
use uuid::Uuid;

pub(super) use socket::attach;
pub(crate) const CONTROL_HEADER: &str = "x-aow-acp-control";

#[derive(Clone, Default)]
pub(crate) struct Controls(Arc<Mutex<HashMap<String, Weak<Controller>>>>);

impl Controls {
    pub(super) fn session(&self, id: &str) -> Arc<Controller> {
        let mut sessions = self.0.lock().unwrap();
        sessions.retain(|_, controller| controller.strong_count() > 0);
        if let Some(controller) = sessions.get(id).and_then(Weak::upgrade) {
            return controller;
        }
        let controller = Arc::new(Controller::default());
        sessions.insert(id.into(), Arc::downgrade(&controller));
        controller
    }
}

#[derive(Clone)]
struct Owner {
    attachment: Uuid,
    token: String,
}

/// A CLI operation owns admission until startup/submission finishes, even if its HTTP client leaves.
pub(super) struct CliLease {
    controller: Arc<Controller>,
    attachment: Uuid,
    pub headers: HeaderMap,
}

impl Drop for CliLease {
    fn drop(&mut self) {
        self.controller.release(self.attachment);
    }
}

pub(super) struct Controller {
    owner: Mutex<Option<Owner>>,
    restore: AsyncMutex<()>,
    changed: watch::Sender<()>,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            owner: Mutex::new(None),
            restore: AsyncMutex::new(()),
            changed: watch::channel(()).0,
        }
    }
}

#[derive(Serialize)]
pub(super) struct ControlState {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

impl Controller {
    pub(super) fn claim_cli(self: &Arc<Self>) -> Result<CliLease, HttpError> {
        let attachment = Uuid::new_v4();
        let state = self.claim(attachment, false);
        let token = state.token.ok_or_else(|| {
            HttpError::new(
                StatusCode::CONFLICT,
                "acp_control_required",
                aow_i18n::text("http.acp_control_conflict"),
                None,
            )
        })?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTROL_HEADER, token.parse().expect("UUID header"));
        Ok(CliLease {
            controller: self.clone(),
            attachment,
            headers,
        })
    }
    fn state_for(owner: &Option<Owner>, attachment: Uuid) -> ControlState {
        let token = owner
            .as_ref()
            .filter(|owner| owner.attachment == attachment)
            .map(|owner| owner.token.clone());
        ControlState {
            state: if token.is_some() {
                "claimed"
            } else {
                "observing"
            },
            token,
        }
    }

    fn state(&self, attachment: Uuid) -> ControlState {
        Self::state_for(&self.owner.lock().unwrap(), attachment)
    }

    fn claim(&self, attachment: Uuid, force: bool) -> ControlState {
        let mut owner = self.owner.lock().unwrap();
        if owner.is_none()
            || force
                && owner
                    .as_ref()
                    .is_some_and(|owner| owner.attachment != attachment)
        {
            *owner = Some(Owner {
                attachment,
                token: Uuid::new_v4().to_string(),
            });
            self.changed.send_replace(());
        }
        Self::state_for(&owner, attachment)
    }

    fn release(&self, attachment: Uuid) {
        let mut owner = self.owner.lock().unwrap();
        if owner
            .as_ref()
            .is_some_and(|owner| owner.attachment == attachment)
        {
            *owner = None;
            self.changed.send_replace(());
        }
    }

    pub(super) async fn authorize_restore(
        &self,
        headers: &HeaderMap,
    ) -> Result<MutexGuard<'_, ()>, HttpError> {
        self.authorize_lifecycle(headers, false).await
    }

    pub(super) async fn authorize_lifecycle(
        &self,
        headers: &HeaderMap,
        allow_unowned: bool,
    ) -> Result<MutexGuard<'_, ()>, HttpError> {
        // Creation/replay and close/delete cannot overtake each other. Permissions
        // and cancellation remain available while the adapter waits for input.
        self.authorize(headers, allow_unowned)?;
        let restore = self.restore.lock().await;
        self.authorize(headers, allow_unowned)?;
        Ok(restore)
    }

    pub(super) fn authorize(
        &self,
        headers: &HeaderMap,
        allow_unowned: bool,
    ) -> Result<(), HttpError> {
        // Takeover revokes new admissions immediately. Already admitted requests
        // may finish, and changing the owner never cancels an active agent task.
        let owner = self.owner.lock().unwrap();
        let token = headers
            .get(CONTROL_HEADER)
            .and_then(|value| value.to_str().ok());
        if owner
            .as_ref()
            .is_some_and(|owner| token == Some(owner.token.as_str()))
            || owner.is_none() && allow_unowned
        {
            Ok(())
        } else {
            Err(HttpError::new(
                StatusCode::CONFLICT,
                "acp_control_required",
                aow_i18n::text("common.controlRequired"),
                None,
            ))
        }
    }
}
