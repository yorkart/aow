use super::*;

#[derive(Default)]
pub(in crate::runtime) struct SpawnTracker {
    pub(in crate::runtime) count: AtomicUsize,
    pub(in crate::runtime) zero: Notify,
}

pub(in crate::runtime) struct SpawnPermit {
    pub(in crate::runtime) tracker: Arc<SpawnTracker>,
}

impl SpawnTracker {
    pub(in crate::runtime) fn begin(self: &Arc<Self>) -> SpawnPermit {
        self.count.fetch_add(1, Ordering::AcqRel);
        SpawnPermit {
            tracker: self.clone(),
        }
    }

    pub(in crate::runtime) async fn wait_zero(&self) {
        loop {
            // `notify_one` stores a permit if the last SpawnPermit is dropped
            // between this count check and polling `notified()`.
            let notified = self.zero.notified();
            if self.count.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
}

impl Drop for SpawnPermit {
    fn drop(&mut self) {
        let previous = self.tracker.count.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous != 0, "spawn permit count underflow");
        if previous == 1 {
            self.tracker.zero.notify_one();
        }
    }
}
