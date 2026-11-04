use std::fmt;

/// Errors are returned without wrapping timestamps, truncating IDs, or resetting state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Node IDs occupy 10 bits and must be in `0..=1023`.
    InvalidNode(u16),
    /// The configured epoch is later than the current wall clock.
    EpochInFuture,
    /// The elapsed timestamp no longer fits in 41 bits.
    TimestampOverflow,
    /// The time source moved backwards relative to the last generated ID.
    ClockMovedBackwards { previous: u64, current: u64 },
    /// Another thread panicked while holding the generator lock.
    Poisoned,
    /// An ID exceeds the positive 63-bit range.
    IdOutOfRange,
    /// An encoded ID is empty, contains an invalid digit, or is not canonical.
    InvalidEncoding,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNode(node) => write!(formatter, "node ID {node} is outside 0..=1023"),
            Self::EpochInFuture => formatter.write_str("epoch is later than the current clock"),
            Self::TimestampOverflow => formatter.write_str("timestamp exceeds 41 bits"),
            Self::ClockMovedBackwards { previous, current } => {
                write!(
                    formatter,
                    "clock moved backwards from {previous} to {current}"
                )
            }
            Self::Poisoned => formatter.write_str("snowflake generator lock is poisoned"),
            Self::IdOutOfRange => formatter.write_str("ID exceeds 63 bits"),
            Self::InvalidEncoding => formatter.write_str("invalid or non-canonical ID encoding"),
        }
    }
}

impl std::error::Error for Error {}
