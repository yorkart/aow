use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_secs(60);
const MAX_DETAILS: usize = 5;

/// One bounded, memory-only budget for the observer, including persistence and
/// cleanup errors. Tracking one entry per damaged file would itself be unbounded.
#[derive(Clone, Default)]
pub(in crate::automations) struct Warnings(Arc<Mutex<Window>>);

#[derive(Default)]
struct Window {
    started: Option<Instant>,
    details: BTreeSet<(&'static str, String, String)>,
    suppressed: u64,
}

impl Window {
    fn rollover(&mut self, now: Instant) -> u64 {
        if self
            .started
            .is_some_and(|start| now.duration_since(start) < INTERVAL)
        {
            return 0;
        }
        self.started = Some(now);
        self.details.clear();
        std::mem::take(&mut self.suppressed)
    }

    fn detail(&mut self, operation: &'static str, task_id: &str, run_id: &str) -> bool {
        if self.details.len() < MAX_DETAILS
            && self
                .details
                .insert((operation, task_id.into(), run_id.into()))
        {
            true
        } else {
            self.suppressed = self.suppressed.saturating_add(1);
            false
        }
    }
}

impl Warnings {
    pub(in crate::automations) fn flush(&self) {
        let suppressed = self.0.lock().unwrap().rollover(Instant::now());
        if suppressed > 0 {
            tracing::warn!(
                suppressed,
                "automation observer warnings suppressed in the last minute"
            );
        }
    }

    pub(in crate::automations) fn report(
        &self,
        operation: &'static str,
        task_id: &str,
        run_id: &str,
        error: &anyhow::Error,
    ) {
        self.flush();
        if self.0.lock().unwrap().detail(operation, task_id, run_id) {
            tracing::warn!(operation, task_id, run_id, error = %format_args!("{error:#}"), "automation observer failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitted_warnings_include_root_causes_and_summarize_suppression() {
        let log = tempfile::NamedTempFile::new().unwrap();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(log.reopen().unwrap())
            .finish();
        let warnings = Warnings::default();
        let error = anyhow::Error::new(std::io::Error::from_raw_os_error(libc::ENOSPC))
            .context("cannot persist notification state");
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..12 {
                for run in 0..390 {
                    warnings.report("inspect run", "task", &run.to_string(), &error);
                }
            }
            warnings.0.lock().unwrap().started = Some(Instant::now() - INTERVAL);
            warnings.flush();
            warnings.flush();
        });
        let output = std::fs::read_to_string(log.path()).unwrap();
        assert_eq!(output.matches("automation observer failed").count(), 5);
        assert_eq!(output.matches("warnings suppressed").count(), 1);
        assert!(output.contains("suppressed=4675"));
        assert!(output.contains("cannot persist notification state"));
        assert!(output.contains(&error.root_cause().to_string()));
    }

    #[test]
    fn thousands_of_failures_share_a_bounded_budget_and_summary() {
        let now = Instant::now();
        let mut window = Window::default();
        assert_eq!(window.rollover(now), 0);
        let mut details = 0;
        for round in 0..12 {
            assert_eq!(window.rollover(now + Duration::from_secs(round * 5)), 0);
            for run in 0..390 {
                details += usize::from(window.detail("inspect", "task", &run.to_string()));
            }
        }
        assert_eq!(details, 5);
        assert_eq!(window.details.len(), 5);
        // Flush also works after recovery, without needing another failure.
        assert_eq!(window.rollover(now + INTERVAL), 4675);
        assert_eq!(window.rollover(now + INTERVAL), 0);
        assert!(window.detail("inspect", "task", "0"));
        assert!(!window.detail("inspect", "task", "0"));
        assert!(window.detail("persist", "task", "0"));
    }
}
