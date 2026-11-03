use crate::facade::ConnectionStatus;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub(crate) struct Progress(
    Arc<Mutex<(ConnectionStatus, Instant)>>,
    Option<(
        crate::facade::events::SyncEvents,
        crate::facade::SyncResource,
    )>,
);

impl Default for Progress {
    fn default() -> Self {
        Self(
            Arc::new(Mutex::new((
                ConnectionStatus {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    phase: "queued".into(),
                    detail: None,
                    running: true,
                    elapsed_seconds: 0,
                },
                Instant::now(),
            ))),
            None,
        )
    }
}

impl Progress {
    pub(crate) fn observed(
        events: crate::facade::events::SyncEvents,
        resource: crate::facade::SyncResource,
    ) -> Self {
        Self(Self::default().0, Some((events, resource)))
    }
    pub(crate) fn notify(&self) {
        if let Some((events, resource)) = &self.1 {
            events.operation_changed(resource.clone(), self.snapshot().operation_id);
        }
    }
    pub(crate) fn set(&self, phase: &str) {
        let mut current = self.0.lock().unwrap();
        if current.0.phase == phase && current.0.detail.is_none() {
            return;
        }
        current.0.phase = phase.into();
        current.0.detail = None;
        drop(current);
        self.notify();
    }
    pub(crate) fn network_error(&self, code: &str) {
        let mut current = self.0.lock().unwrap();
        if current.0.detail.as_deref() == Some(code) {
            return;
        }
        current.0.detail = Some(code.into());
        drop(current);
        self.notify();
    }
    pub(crate) fn finish(&self, success: bool) {
        let mut current = self.0.lock().unwrap();
        let phase = if success { "ready" } else { "failed" };
        if !current.0.running && current.0.phase == phase {
            return;
        }
        current.0.phase = phase.into();
        if success {
            current.0.detail = None;
        }
        current.0.running = false;
        current.0.elapsed_seconds = current.1.elapsed().as_secs();
        drop(current);
        self.notify();
    }
    pub(crate) fn snapshot(&self) -> ConnectionStatus {
        let current = self.0.lock().unwrap();
        let mut status = current.0.clone();
        if status.running {
            status.elapsed_seconds = current.1.elapsed().as_secs();
        }
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_notifies_committed_phases_and_both_final_results() {
        for success in [true, false] {
            let events = crate::facade::events::SyncEvents::default();
            let mut receiver = events.subscribe();
            let resource = crate::facade::SyncResource::Operation {
                agent_id: "test".into(),
                cwd: None,
            };
            let progress = Progress::observed(events, resource.clone());
            progress.set("installing");
            assert_eq!(receiver.try_recv().unwrap().resource, resource);
            assert!(progress.snapshot().running);
            progress.network_error("timeout");
            receiver.try_recv().unwrap();
            assert_eq!(progress.snapshot().detail.as_deref(), Some("timeout"));
            progress.finish(success);
            receiver.try_recv().unwrap();
            assert!(!progress.snapshot().running);
            assert_eq!(
                progress.snapshot().phase,
                if success { "ready" } else { "failed" }
            );
            assert!(receiver.try_recv().is_err());
        }
    }
}
