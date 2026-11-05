//! Read synchronization is independent of the session control channel.
use super::SessionSnapshot;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

pub use aow_host_contract::acp::{SessionVersion, SyncEvent, SyncResource};
impl From<&SessionSnapshot> for SessionVersion {
    fn from(thread: &SessionSnapshot) -> Self {
        Self {
            generation: thread.sync_generation.clone(),
            revision: thread.revision,
            info: thread.into(),
        }
    }
}

struct State {
    revision: u64,
    latest: HashMap<SyncResource, SyncEvent>,
}

#[derive(Clone)]
pub(crate) struct SyncEvents {
    boot_id: Arc<String>,
    state: Arc<Mutex<State>>,
    sender: broadcast::Sender<SyncEvent>,
    protocol: broadcast::Sender<serde_json::Value>,
}

impl Default for SyncEvents {
    fn default() -> Self {
        Self {
            boot_id: Arc::new(uuid::Uuid::new_v4().to_string()),
            state: Arc::new(Mutex::new(State {
                revision: 0,
                latest: HashMap::new(),
            })),
            sender: broadcast::channel(512).0,
            protocol: broadcast::channel(2048).0,
        }
    }
}

impl SyncEvents {
    pub(crate) fn subscribe_protocol(&self) -> broadcast::Receiver<serde_json::Value> {
        self.protocol.subscribe()
    }
    pub(crate) fn protocol(&self, value: serde_json::Value) {
        let _ = self.protocol.send(value);
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.sender.subscribe()
    }

    pub(crate) fn current(&self, resource: &SyncResource) -> SyncEvent {
        self.state
            .lock()
            .unwrap()
            .latest
            .get(resource)
            .cloned()
            .unwrap_or_else(|| SyncEvent {
                resource: resource.clone(),
                boot_id: (*self.boot_id).clone(),
                version: 0,
                deleted: false,
                urgent: true,
                operation_id: None,
                session: None,
            })
    }

    pub(crate) fn changed(&self, resource: SyncResource) {
        self.publish(resource, None, false, true);
    }

    pub(crate) fn publish(
        &self,
        resource: SyncResource,
        session: Option<SessionVersion>,
        deleted: bool,
        urgent: bool,
    ) {
        self.publish_inner(resource, session, deleted, urgent, None);
    }

    pub(crate) fn operation_changed(&self, resource: SyncResource, operation_id: String) {
        self.publish_inner(resource, None, false, true, Some(operation_id));
    }

    fn publish_inner(
        &self,
        resource: SyncResource,
        session: Option<SessionVersion>,
        deleted: bool,
        urgent: bool,
        operation_id: Option<String>,
    ) {
        let mut state = self.state.lock().unwrap();
        state.revision += 1;
        let event = SyncEvent {
            resource: resource.clone(),
            boot_id: (*self.boot_id).clone(),
            version: state.revision,
            deleted,
            urgent,
            operation_id,
            session,
        };
        if deleted {
            state.latest.remove(&resource);
        } else {
            state.latest.insert(resource, event.clone());
        }
        // This bounded channel never waits on a browser. A lagged subscriber
        // must reconnect and revalidate all resources rather than skip changes.
        let _ = self.sender.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_subscribers_have_a_bounded_queue_and_restart_changes_epoch() {
        let events = SyncEvents::default();
        let mut slow = events.subscribe();
        let resource = SyncResource::Sessions {
            cwd: "/repo".into(),
        };
        for _ in 0..1024 {
            events.changed(resource.clone());
        }
        assert!(matches!(
            slow.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
        assert_eq!(events.current(&resource).version, 1024);
        assert_ne!(
            events.current(&resource).boot_id,
            SyncEvents::default().current(&resource).boot_id
        );
    }
}
