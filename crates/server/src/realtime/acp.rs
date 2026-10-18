//! Per-socket subscriptions and bounded coalescing of committed ACP changes.
use aow_acp::{AcpService, SyncEvent, SyncResource};
use serde::Deserialize;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Command {
    Subscribe { id: String, resource: SyncResource },
    Unsubscribe { id: String },
}

#[derive(Default)]
pub(super) struct Subscriptions {
    active: HashMap<String, SyncResource>,
    pending: HashMap<SyncResource, (Instant, SyncEvent)>,
}

impl Subscriptions {
    pub(super) fn command(
        &mut self,
        command: Command,
        service: &AcpService,
    ) -> Result<Option<serde_json::Value>, ()> {
        match command {
            Command::Subscribe { id, resource } => {
                if id.len() > 128 || (!self.active.contains_key(&id) && self.active.len() >= 256) {
                    return Err(());
                }
                // Register before reading the head. The broadcast receiver was
                // created before the WebSocket upgrade, retaining racing commits.
                self.active.insert(id.clone(), resource.clone());
                self.pending
                    .retain(|resource, _| self.active.values().any(|active| active == resource));
                let head = service.sync_event(&resource);
                if head.deleted {
                    self.active.remove(&id);
                }
                Ok(Some(
                    serde_json::json!({"type": "ready", "id": id, "head": head}),
                ))
            }
            Command::Unsubscribe { id } => {
                self.active.remove(&id);
                self.pending
                    .retain(|resource, _| self.active.values().any(|active| active == resource));
                Ok(None)
            }
        }
    }

    pub(super) fn change(&mut self, change: SyncEvent) -> Option<serde_json::Value> {
        if !self
            .active
            .values()
            .any(|resource| *resource == change.resource)
        {
            return None;
        }
        if change.deleted
            || change.urgent && !matches!(change.resource, SyncResource::Sessions { .. })
        {
            self.pending.remove(&change.resource);
            return Some(self.message(change));
        }
        let delay = if matches!(change.resource, SyncResource::Sessions { .. }) {
            500
        } else {
            50
        };
        self.pending
            .entry(change.resource.clone())
            .and_modify(|(_, value)| *value = change.clone())
            .or_insert((Instant::now() + Duration::from_millis(delay), change));
        None
    }

    fn message(&mut self, head: SyncEvent) -> serde_json::Value {
        let ids: Vec<_> = self
            .active
            .iter()
            .filter(|(_, resource)| **resource == head.resource)
            .map(|(id, _)| id.clone())
            .collect();
        if head.deleted {
            self.active.retain(|_, resource| *resource != head.resource);
        }
        serde_json::json!({"type": "change", "ids": ids, "head": head})
    }

    pub(super) fn due(&mut self) -> Vec<serde_json::Value> {
        let now = Instant::now();
        let due: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (deadline, _))| *deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        due.into_iter()
            .filter_map(|key| {
                self.pending
                    .remove(&key)
                    .map(|(_, change)| self.message(change))
            })
            .collect()
    }
}
