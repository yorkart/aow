//! Unified Logging adapter for macOS LaunchAgents. Foreground commands keep
//! their normal tracing subscriber; no process-wide stdio redirection is used.
#![cfg(target_os = "macos")]

use std::{
    backtrace::{Backtrace, BacktraceStatus},
    ffi::{CStr, CString, c_char, c_void},
    io::{self, Write},
    ptr::NonNull,
    sync::{Arc, OnceLock},
};

use tracing::{Level, Metadata};
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

unsafe extern "C" {
    fn aow_macos_log_create(category: *const c_char) -> *mut c_void;
    fn aow_macos_log_release(log: *mut c_void);
    fn aow_macos_log_write(log: *mut c_void, level: u8, message: *const c_char);
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum LogType {
    Default = 0,
    Debug = 1,
    Error = 2,
    Fault = 3,
}

struct Handle(NonNull<c_void>);
// os_log objects support concurrent logging; the object is immutable here and
// released only when the final Arc is dropped, after all borrowed writers.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { aow_macos_log_release(self.0.as_ptr()) };
    }
}

#[derive(Clone)]
struct Logger(Arc<Handle>);

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
    // Apple's API always returns a valid object and copies category/subsystem.
    let handle = unsafe { aow_macos_log_create(category.as_ptr()) };
    let logger = Logger(Arc::new(Handle(
        NonNull::new(handle).expect("os_log_create returned null"),
    )));
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

impl Logger {
    fn emit(&self, level: LogType, message: &str) {
        // Bound each os_log dynamic string to avoid its normal message-size
        // truncation. Keep long error chains/backtraces and UTF-8 intact.
        let message = message.replace('\0', "\\0");
        let mut rest = message.trim_end_matches('\n');
        let mut continued = false;
        while !rest.is_empty() {
            let mut end = rest.len().min(768);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let chunk = &rest[..end];
            let text = if continued {
                CString::new(format!("[continued] {chunk}"))
            } else {
                CString::new(chunk)
            }
            .expect("NULs were escaped above");
            unsafe {
                aow_macos_log_write(self.0.0.as_ptr(), level as u8, text.as_ptr());
            }
            continued = true;
            rest = &rest[end..];
        }
    }
}

struct EventWriter<'a> {
    logger: &'a Logger,
    level: LogType,
    bytes: Vec<u8>,
}

impl<'a> MakeWriter<'a> for Logger {
    type Writer = EventWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        EventWriter {
            logger: self,
            level: LogType::Default,
            bytes: Vec::new(),
        }
    }

    fn make_writer_for(&'a self, metadata: &Metadata<'_>) -> Self::Writer {
        EventWriter {
            logger: self,
            bytes: Vec::new(),
            level: match *metadata.level() {
                Level::ERROR => LogType::Error,
                Level::DEBUG | Level::TRACE => LogType::Debug,
                // Apple's Info is normally memory-only. Keep our existing
                // info/warn diagnostics eligible for persistence using Default.
                Level::INFO | Level::WARN => LogType::Default,
            },
        }
    }
}

impl Write for EventWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for EventWriter<'_> {
    fn drop(&mut self) {
        self.logger
            .emit(self.level, &String::from_utf8_lossy(&self.bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Invoked by the opt-in native log test with a unique marker; no service or
    // global log configuration is modified by the fixture.
    #[test]
    #[ignore = "native subprocess fixture; see scripts/tests/unified-logging.test.mjs"]
    fn native_log_fixture() {
        let Ok(marker) = std::env::var("AOW_LOG_TEST_MARKER") else {
            return;
        };
        assert!(init_from_env(c"test", "info"));
        let span = tracing::info_span!("fixture", run = %marker);
        let _entered = span.enter();
        tracing::info!("info marker {marker}");
        tracing::warn!("warn marker {marker}");
        tracing::error!("error marker {marker}");
        tracing::info!("UTF-8 中文 🦀 NUL:\0 END {marker}");
        report_error(&format!(
            "long {marker} {} tail {marker}",
            "诊断🦀".repeat(400)
        ));
        report_error(&format!("fatal marker {marker}"));
        let _ = std::panic::catch_unwind(|| panic!("panic marker {marker}"));
    }
}
