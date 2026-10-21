use std::time::Duration;

/// Login session limits, configured once when the server starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionTimeouts {
    pub(super) max_age: Duration,
    pub(super) idle_timeout: Duration,
}

impl SessionTimeouts {
    pub const DEFAULT_MAX_AGE_SECONDS: u64 = 12 * 60 * 60;
    pub const DEFAULT_IDLE_TIMEOUT_SECONDS: u64 = 30 * 60;

    pub fn from_seconds(max_age: u64, idle_timeout: u64) -> anyhow::Result<Self> {
        anyhow::ensure!(max_age > 0, "session max age must be greater than zero");
        anyhow::ensure!(
            idle_timeout > 0,
            "session idle timeout must be greater than zero"
        );
        Ok(Self {
            max_age: Duration::from_secs(max_age),
            idle_timeout: Duration::from_secs(idle_timeout),
        })
    }
}

impl Default for SessionTimeouts {
    fn default() -> Self {
        Self::from_seconds(
            Self::DEFAULT_MAX_AGE_SECONDS,
            Self::DEFAULT_IDLE_TIMEOUT_SECONDS,
        )
        .expect("positive default session timeouts")
    }
}
