use std::io;

use crate::platform;

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
