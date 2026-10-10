use crate::facade::ConnectionStatus;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub(crate) struct Progress(Arc<Mutex<(ConnectionStatus, Instant)>>);

impl Default for Progress {
    fn default() -> Self {
        Self(Arc::new(Mutex::new((
            ConnectionStatus {
                phase: "queued".into(),
                detail: None,
                running: true,
                elapsed_seconds: 0,
            },
            Instant::now(),
        ))))
    }
}

impl Progress {
    pub(crate) fn set(&self, phase: &str) {
        let mut current = self.0.lock().unwrap();
        current.0.phase = phase.into();
        current.0.detail = None;
    }
    pub(crate) fn network_error(&self, code: &str) {
        self.0.lock().unwrap().0.detail = Some(code.into());
    }
    pub(crate) fn finish(&self, success: bool) {
        let mut current = self.0.lock().unwrap();
        current.0.phase = if success { "ready" } else { "failed" }.into();
        current.0.running = false;
        current.0.elapsed_seconds = current.1.elapsed().as_secs();
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
