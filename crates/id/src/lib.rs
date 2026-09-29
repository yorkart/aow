//! Snowflake IDs using the layout and monotonic clock approach of bwmarrin/snowflake.
//!
//! Share one [`Generator`] per node ID. Independently running generators must use
//! distinct node IDs, including processes on the same machine. No node allocation
//! or persistent state is managed by this crate.
//!
//! ```
//! use aow_id::{Generator, Snowflake};
//!
//! let generator = Generator::new(1)?;
//! let id = generator.generate()?;
//! let text = id.to_string(); // Base62, at most 11 ASCII characters.
//! assert_eq!(text.parse::<Snowflake>()?, id);
//! # Ok::<(), aow_id::Error>(())
//! ```
//!
//! The clock is anchored to wall time at construction and advances using
//! [`std::time::Instant`]. Wall-clock adjustments during this generator's lifetime
//! do not change that anchor. This does not protect against wall-clock rollback
//! across restarts.

mod clock;
mod error;
mod generator;
mod id;

pub use error::Error;
pub use generator::{Generator, TWITTER_EPOCH_MILLIS};
pub use id::Snowflake;
