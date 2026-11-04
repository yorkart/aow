//! Shared language resources for user-facing messages. English is the default.

mod language;
mod resources;

pub use language::{Language, current_language, scope, spawn, spawn_blocking, with_language};
pub use resources::{message, text};

#[cfg(test)]
mod tests;
