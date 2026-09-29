use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

use anyhow::{Context, Result, ensure};

pub fn valid_component(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 96
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
        "无效的任务或执行 ID"
    );
    Ok(())
}

/// Allocate a compact Snowflake ID for a new execution.
pub fn new_run_id() -> String {
    aow_id::new_id()
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "数据目录不能是符号链接"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("文件缺少父目录")?;
    if !parent.exists() {
        private_dir(parent)?;
    }
    let temporary = parent.join(format!(".{}.tmp", aow_id::new_id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
