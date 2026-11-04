//! Unified Logging adapter for macOS LaunchAgents. Foreground commands keep
//! their normal tracing subscriber; no process-wide stdio redirection is used.
#![cfg(target_os = "macos")]

mod init;
mod logger;
mod os_log;
mod writer;

pub use init::{init_from_env, report_error, requested};

#[cfg(test)]
mod tests;
