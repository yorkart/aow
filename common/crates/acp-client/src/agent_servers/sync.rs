use super::AgentServerStore;
use crate::facade::{SessionVersion, SyncEvent, SyncResource};

impl AgentServerStore {
    pub fn subscribe_sync(&self) -> tokio::sync::broadcast::Receiver<SyncEvent> {
        self.inner.events.subscribe()
    }

    /// Called after the receiver is installed. No adapter is started by a read.
    pub fn sync_event(&self, resource: &SyncResource) -> SyncEvent {
        match resource {
            SyncResource::Session { thread_id } => {
                let mut threads = self.inner.threads.lock().unwrap();
                match threads.thread(thread_id) {
                    Ok(thread) => {
                        let thread = thread.lock().unwrap();
                        let mut event = self.inner.events.current(resource);
                        event.session = Some(SessionVersion::from(&*thread));
                        event
                    }
                    Err(_) => {
                        let mut event = self.inner.events.current(resource);
                        event.deleted = threads.session_info(thread_id).is_err();
                        event
                    }
                }
            }
            SyncResource::Connection { thread_id } => {
                let threads = self.inner.threads.lock().unwrap();
                let mut event = self.inner.events.current(resource);
                event.deleted = threads.session_info(thread_id).is_err();
                event
            }
            SyncResource::Requests { connection_id } => {
                let mut event = self.inner.events.current(resource);
                event.deleted = !self
                    .connection(connection_id)
                    .is_ok_and(|connection| connection.connected());
                event
            }
            _ => self.inner.events.current(resource),
        }
    }
}
