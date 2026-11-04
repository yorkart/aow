use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
    sync::LazyLock,
    time::Duration,
};

use crate::{Generator, Snowflake, generator::MAX_NODE};

static LOCAL: LazyLock<LocalGenerator> = LazyLock::new(|| {
    // Use a fixed per-account directory, independent of TMPDIR and state-dir,
    // so the server, terminal daemon and concurrent CLI processes coordinate.
    // SAFETY: geteuid has no preconditions and does not retain pointers.
    let uid = unsafe { libc::geteuid() };
    let directory = Path::new("/tmp").join(format!("aow-id-{uid}"));
    prepare_directory(&directory, uid).expect("cannot prepare Snowflake node directory");
    LocalGenerator::acquire(&directory).expect("cannot acquire a local Snowflake node")
});

/// Allocate an application ID from this process's shared Snowflake generator.
///
/// Processes under the same OS account lease distinct nodes with file locks.
/// No sequence is persisted. This does not protect against clock rollback
/// across process restarts, and does not coordinate separate machines/accounts.
/// IDs are identifiers, not unpredictable authentication or sharing secrets.
///
/// # Panics
/// Panics if the node directory is unavailable, all 1024 nodes are occupied,
/// or the clock can no longer produce a valid ID. Never falls back to randomness.
pub fn new_id() -> String {
    new_snowflake().to_string()
}

/// Allocate the numeric Snowflake value using the same generator as [`new_id`].
/// Has the same initialization and failure conditions as [`new_id`].
pub fn new_snowflake() -> Snowflake {
    LOCAL
        .generator
        .generate()
        .expect("cannot generate Snowflake ID")
}

struct LocalGenerator {
    generator: Generator,
    // Keep the descriptor locked until process exit. Do not unlink lock files:
    // other processes must always lock the same inode for a particular node.
    _lease: File,
}

impl LocalGenerator {
    fn acquire(directory: &Path) -> io::Result<Self> {
        let start = std::process::id() % (u32::from(MAX_NODE) + 1);
        for offset in 0..=u32::from(MAX_NODE) {
            let node = ((start + offset) % (u32::from(MAX_NODE) + 1)) as u16;
            let lease = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(directory.join(format!("{node}.lock")))?;
            match lease.try_lock() {
                Ok(()) => {
                    // A previous holder may have exited during this millisecond.
                    // Start beyond it before resetting the in-memory sequence.
                    std::thread::sleep(Duration::from_millis(2));
                    return Ok(Self {
                        generator: Generator::new(node).map_err(io::Error::other)?,
                        _lease: lease,
                    });
                }
                Err(std::fs::TryLockError::WouldBlock) => continue,
                Err(std::fs::TryLockError::Error(error)) => return Err(error),
            }
        }
        Err(io::Error::other("all 1024 Snowflake nodes are occupied"))
    }
}

fn prepare_directory(directory: &Path, uid: u32) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Snowflake node directory must be private and owned by the current account",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
