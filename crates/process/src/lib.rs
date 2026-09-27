//! Read-only OS process inspection shared by terminald and the web server.
//! Agent recognition and terminal foreground selection belong to callers.

use std::{io, path::PathBuf};

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(any(target_os = "macos", test))]
mod procargs;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use unsupported as platform;

pub use platform::{command, cwd, info, list_pids};

/// Platform-neutral metadata. A zero tty means no controlling terminal.
#[derive(Debug)]
pub struct ProcessInfo {
    pub pid: i32,
    pub parent: i32,
    pub group: i32,
    pub session: i32,
    pub tty: i32,
    pub foreground: i32,
    /// Unix process state, including Z (zombie) and T/t (stopped).
    pub state: char,
    /// Opaque OS start identity; Linux keeps its existing /proc clock ticks.
    pub start_time: String,
}

/// Entrypoints used only for local recognition. Never log or publish arguments.
#[derive(Default)]
pub struct ProcessCommand {
    pub executable: Option<PathBuf>,
    /// NUL-separated arguments, with their original boundaries preserved.
    pub arguments: Vec<u8>,
}

const COMMAND_LIMIT: u64 = 16 * 1024;
const ENVIRONMENT_LIMIT: u64 = 1024 * 1024;

pub fn same_process(pid: i32, start_time: &str) -> bool {
    pid > 0 && platform::start_time(pid).is_ok_and(|current| current == start_time)
}

/// Read an environment only while the PID still identifies the expected process.
/// Callers must select their configuration allowlist before retaining any data.
pub fn environment(pid: i32, start_time: &str) -> io::Result<Vec<u8>> {
    let changed = || io::Error::new(io::ErrorKind::NotFound, "process identity changed");
    if !same_process(pid, start_time) {
        return Err(changed());
    }
    let environment = platform::environment(pid)?;
    if !same_process(pid, start_time) {
        return Err(changed());
    }
    Ok(environment)
}
