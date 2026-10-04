use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("path must be absolute: {0}")]
    PathNotAbsolute(String),
    #[error("path is not a directory: {0}")]
    NotDirectory(String),
    #[error("path is not a regular file: {0}")]
    NotFile(String),
    #[error("file is too large to edit ({size} bytes, maximum {maximum}): {path}")]
    TooLarge {
        path: String,
        size: u64,
        maximum: u64,
    },
    #[error("file is not UTF-8 text: {0}")]
    NotUtf8(String),
    #[error("file contains NUL bytes: {0}")]
    Binary(String),
    #[error("file changed on disk (expected {expected}, current {current}): {path}")]
    VersionConflict {
        path: String,
        expected: String,
        current: String,
    },
    #[error("invalid file name: {0}")]
    InvalidFileName(String),
    #[error("destination already exists: {0}")]
    AlreadyExists(String),
    #[error("streaming request failed: {0}")]
    Stream(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
