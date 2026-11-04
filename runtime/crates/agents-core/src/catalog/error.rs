//! Session failures classified independently of their transport.
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub enum SessionErrorKind {
    Invalid,
    NotFound,
    Unavailable,
    Conflict,
    Internal,
}

#[derive(Debug)]
pub struct SessionError {
    pub kind: SessionErrorKind,
    message: String,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SessionError {}

pub(super) fn error(kind: SessionErrorKind, message: impl fmt::Display) -> SessionError {
    SessionError {
        kind,
        message: message.to_string(),
    }
}
