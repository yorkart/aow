use thiserror::Error;

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("invalid terminal runtime request: {0}")]
    Invalid(String),
    #[error("terminal runtime not found: {0}")]
    NotFound(String),
    #[error("terminal runtime conflict: {0}")]
    Conflict(String),
    #[error("failed to start terminal runtime: {0}")]
    Pty(String),
    #[error("terminal runtime worker failed: {0}")]
    Worker(String),
    #[error("terminal state lock is poisoned")]
    Poisoned,
    #[error("terminal runtime is shutting down")]
    ShuttingDown,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
