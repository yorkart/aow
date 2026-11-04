//! Snowflake IDs using the layout and monotonic clock approach of bwmarrin/snowflake.
//!
//! Use [`new_id`] for application IDs: it shares a generator within the process
//! and leases a distinct node for each process under the same OS account.
//! The default encoding is lowercase Base36 (at most 13 characters), suitable
//! for both case-sensitive and case-insensitive filesystems.
//!
//! When managing [`Generator`] directly, share one generator per node ID and
//! assign distinct nodes to independently running generators. Sequence state
//! is held in memory; no persistent counter is maintained.
//!
//! ```
//! use aow_id::{Generator, Snowflake};
//!
//! let generator = Generator::new(1)?;
//! let id = generator.generate()?;
//! let text = id.to_string(); // Lowercase Base36, at most 13 ASCII characters.
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
mod local;
mod validation;

pub use error::Error;
pub use generator::{Generator, TWITTER_EPOCH_MILLIS};
pub use id::Snowflake;
pub use local::{new_id, new_snowflake};
pub use validation::is_valid_id;
