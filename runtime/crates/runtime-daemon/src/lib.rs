//! One node service hosting native ACP and PTY runtimes on an authenticated HTTP listener.

mod auth;
mod config;
mod service;

pub use config::{Config, load_token};
pub use service::{Options, run_with_listener, run_with_listener_and_vt_config, run_with_shutdown};
