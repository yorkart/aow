use std::{fs::File, os::fd::AsRawFd};

use anyhow::Result;

pub(crate) fn try_exclusive(file: &File) -> Result<bool> {
    try_lock(file, libc::LOCK_EX)
}

pub(super) fn try_shared(file: &File) -> Result<bool> {
    try_lock(file, libc::LOCK_SH)
}

pub(crate) fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

fn try_lock(file: &File, operation: libc::c_int) -> Result<bool> {
    if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(error.into())
    }
}
/// Release advisory locks explicitly: another thread can fork between acquiring
/// a lock and dropping its owner, briefly inheriting the open file description.
pub(super) struct FileLock {
    pub(super) file: File,
    pub(super) owns_lock: bool,
}
impl Drop for FileLock {
    fn drop(&mut self) {
        if self.owns_lock {
            unsafe {
                libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}
impl std::ops::Deref for FileLock {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl std::ops::DerefMut for FileLock {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.file
    }
}
