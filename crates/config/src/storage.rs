use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

pub(super) fn copy_json(source: &Path, destination: &Path) -> Result<(PathBuf, Vec<u8>)> {
    ensure!(
        fs::symlink_metadata(source)?.is_file(),
        "配置必须是普通文件：{}",
        source.display()
    );
    let bytes = fs::read(source)?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .with_context(|| format!("原配置 JSON 无效：{}", source.display()))?;
    atomic_write(destination, &bytes)?;
    Ok((source.to_path_buf(), bytes))
}
pub(super) struct FileLock(File);

impl FileLock {
    pub(super) fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Ok(Self(file));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("配置文件缺少父目录")?;
    private_directories(parent)?;
    let temporary = parent.join(format!(".{}.tmp", aow_id::new_id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn private_directories(path: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}
