// Adapted from bwmarrin/snowflake, commit bc74ab286f15f6c73896cb40ad39c502263c7af3.

use std::sync::Mutex;

use crate::{Error, Snowflake, clock::Clock};

/// The upstream Twitter epoch: 2010-11-04 01:42:54.657 UTC.
pub const TWITTER_EPOCH_MILLIS: u64 = 1_288_834_974_657;

pub(crate) const SEQUENCE_BITS: u32 = 12;
pub(crate) const NODE_BITS: u32 = 10;
pub(crate) const TIME_SHIFT: u32 = NODE_BITS + SEQUENCE_BITS;
pub(crate) const MAX_NODE: u16 = (1 << NODE_BITS) - 1;
pub(crate) const MAX_SEQUENCE: u16 = (1 << SEQUENCE_BITS) - 1;
pub(crate) const MAX_TIMESTAMP: u64 = (1 << 41) - 1;

/// A shared, thread-safe generator with a fixed `41 + 10 + 12` bit layout.
///
/// Each independently running generator must have a different node ID. Share this
/// value by reference or `Arc`; creating a second generator with the same node ID
/// can produce duplicate IDs. Epoch and node assignments must remain compatible
/// with previously stored IDs across restarts.
pub struct Generator {
    node_id: u16,
    epoch_millis: u64,
    clock: Clock,
    state: Mutex<State>,
}

impl Generator {
    /// Construct a generator using the upstream Twitter epoch.
    pub fn new(node_id: u16) -> Result<Self, Error> {
        Self::with_epoch(node_id, TWITTER_EPOCH_MILLIS)
    }

    /// Construct a generator with a fixed custom Unix epoch in milliseconds.
    ///
    /// The epoch must not be reset to the current time on each startup: it defines
    /// the meaning of the timestamp bits and must be the same for all generators
    /// participating in the same ID namespace.
    pub fn with_epoch(node_id: u16, epoch_millis: u64) -> Result<Self, Error> {
        if node_id > MAX_NODE {
            return Err(Error::InvalidNode(node_id));
        }
        let clock = Clock::new(epoch_millis)?;
        if clock.elapsed_millis()? > MAX_TIMESTAMP {
            return Err(Error::TimestampOverflow);
        }
        Ok(Self {
            node_id,
            epoch_millis,
            clock,
            state: Mutex::new(State::default()),
        })
    }

    pub fn node_id(&self) -> u16 {
        self.node_id
    }

    pub fn epoch_millis(&self) -> u64 {
        self.epoch_millis
    }

    /// Generate an ID, waiting for the next millisecond if the sequence is full.
    ///
    /// This is synchronous. Exhaustion yields the OS thread while holding the
    /// generator lock. It does not provide an asynchronous Tokio wait operation.
    pub fn generate(&self) -> Result<Snowflake, Error> {
        self.generate_with(|| self.clock.elapsed_millis())
    }

    fn generate_with(
        &self,
        mut now: impl FnMut() -> Result<u64, Error>,
    ) -> Result<Snowflake, Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        loop {
            if let Some(id) = state.next(now()?, self.node_id)? {
                return Ok(id);
            }
            // Unlike the upstream tight spin loop, let other runnable threads
            // make progress while this millisecond's sequence is exhausted.
            std::thread::yield_now();
        }
    }
}

#[derive(Default)]
struct State {
    timestamp: u64,
    sequence: u16,
}

impl State {
    fn next(&mut self, timestamp: u64, node_id: u16) -> Result<Option<Snowflake>, Error> {
        if timestamp > MAX_TIMESTAMP {
            return Err(Error::TimestampOverflow);
        }
        if timestamp < self.timestamp {
            return Err(Error::ClockMovedBackwards {
                previous: self.timestamp,
                current: timestamp,
            });
        }
        let sequence = if timestamp == self.timestamp {
            if self.sequence == MAX_SEQUENCE {
                return Ok(None);
            }
            self.sequence + 1
        } else {
            0
        };
        let id = Snowflake::from_u64(
            (timestamp << TIME_SHIFT) | (u64::from(node_id) << SEQUENCE_BITS) | u64::from(sequence),
        )?;
        self.timestamp = timestamp;
        self.sequence = sequence;
        Ok(Some(id))
    }
}

#[cfg(test)]
mod tests;
