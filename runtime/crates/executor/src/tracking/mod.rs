//! Process registration is independent of task scheduling and business concurrency.
mod process;
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};
#[derive(Deserialize)]
struct Process {
    process_group: i32,
    started_before: u64,
}
struct TaskLock {
    file: File,
}
pub(crate) struct ConcurrencySlot {
    lock: TaskLock,
}
impl ConcurrencySlot {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        use std::os::fd::AsRawFd;
        ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Execution is already owned"
        );
        file.set_len(0)?;
        writeln!(
            file,
            "{}",
            serde_json::json!({"boot":process::boot_identity()?})
        )?;
        file.sync_data()?;
        Ok(Self {
            lock: TaskLock { file },
        })
    }
    pub(crate) fn recover(path: &Path) -> Result<()> {
        let mut contents = String::new();
        match File::open(path) {
            Ok(file) => {
                file.take(65537).read_to_string(&mut contents)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        ensure!(
            contents.len() <= 65536,
            "Execution owner record exceeds limit"
        );
        let mut lines = contents.lines();
        let owner: serde_json::Value = serde_json::from_str(lines.next().unwrap_or("{}"))?;
        if owner["boot"].as_str() != Some(process::boot_identity()?.as_str()) {
            return Ok(());
        }
        for line in lines {
            if let Ok(group) = serde_json::from_str::<Process>(line)
                && process::group_alive(&group)?
            {
                unsafe {
                    libc::kill(-group.process_group, libc::SIGKILL);
                }
            }
        }
        Ok(())
    }
}
