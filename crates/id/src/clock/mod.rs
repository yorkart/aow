use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::Error;

pub(crate) struct Clock {
    base: Duration,
    started: Instant,
}

impl Clock {
    pub(crate) fn new(epoch_millis: u64) -> Result<Self, Error> {
        let wall_time = SystemTime::now();
        let started = Instant::now();
        let base = wall_time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| elapsed.checked_sub(Duration::from_millis(epoch_millis)))
            .ok_or(Error::EpochInFuture)?;
        Ok(Self { base, started })
    }

    pub(crate) fn elapsed_millis(&self) -> Result<u64, Error> {
        self.elapsed_with(self.started.elapsed())
    }

    fn elapsed_with(&self, elapsed: Duration) -> Result<u64, Error> {
        // Retain sub-millisecond precision in the anchor, just as Go's time.Time
        // does. Truncate only after adding the monotonic elapsed duration.
        let elapsed = self
            .base
            .checked_add(elapsed)
            .ok_or(Error::TimestampOverflow)?;
        u64::try_from(elapsed.as_millis()).map_err(|_| Error::TimestampOverflow)
    }
}

#[cfg(test)]
mod tests;
