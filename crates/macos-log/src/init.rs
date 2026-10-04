use std::{
    backtrace::{Backtrace, BacktraceStatus},
    ffi::CStr,
    sync::OnceLock,
};

use tracing_subscriber::EnvFilter;

use super::{logger::Logger, os_log::LogType};

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Only launchd sets this flag. Remove it from environments of interactive PTYs.
pub fn requested() -> bool {
    std::env::var_os("AOW_LOG_MODE").is_some_and(|value| value == "unified")
}

/// Initialize before constructing the async runtime, so startup errors and
/// panics have the same destination as ordinary service events.
pub fn init_from_env(category: &CStr, default_filter: &str) -> bool {
    if !requested() {
        return false;
    }
    let logger = Logger::new(category);
    assert!(
        LOGGER.set(logger.clone()).is_ok(),
        "logging initialized twice"
    );
    let previous = std::panic::take_hook();
    let panic_log = logger.clone();
    std::panic::set_hook(Box::new(move |info| {
        panic_log.emit(LogType::Fault, &format!("panic: {info}"));
        let backtrace = Backtrace::capture();
        if backtrace.status() == BacktraceStatus::Captured {
            panic_log.emit(LogType::Fault, &backtrace.to_string());
        }
        previous(info);
    }));
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter)),
        )
        .with_ansi(false)
        .without_time()
        .with_writer(logger)
        .init();
    true
}

/// Fatal startup/shutdown errors must remain visible even with RUST_LOG=off.
pub fn report_error(message: &str) {
    if let Some(logger) = LOGGER.get() {
        logger.emit(LogType::Error, message);
    }
}
