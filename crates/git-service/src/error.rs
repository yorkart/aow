use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("path must be absolute: {0}")]
    PathNotAbsolute(String),
    #[error("path is not inside a Git repository: {0}")]
    NotRepository(String),
    #[error("git command timed out")]
    Timeout,
    #[error("git command failed: {0}")]
    Command(String),
    #[error("git output was not valid UTF-8")]
    InvalidUtf8,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),
}
