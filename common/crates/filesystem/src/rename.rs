use std::path::{Path, PathBuf};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{ffi::CString, os::unix::ffi::OsStrExt};
use tokio::fs;

use super::{
    FsError,
    paths::{absolute_path, display_path},
};

pub async fn rename_file(
    source: impl AsRef<Path>,
    requested_name: &str,
) -> Result<PathBuf, FsError> {
    let source = absolute_path(source)?;
    let metadata = fs::symlink_metadata(&source).await?;
    if !metadata.file_type().is_file() {
        return Err(FsError::NotFile(display_path(&source)));
    }
    rename_entry_path(source, requested_name).await
}

pub async fn rename_entry(
    source: impl AsRef<Path>,
    requested_name: &str,
) -> Result<PathBuf, FsError> {
    let source = absolute_path(source)?;
    let metadata = fs::symlink_metadata(&source).await?;
    if !metadata.file_type().is_file() && !metadata.file_type().is_dir() {
        return Err(FsError::NotFile(display_path(&source)));
    }
    rename_entry_path(source, requested_name).await
}

async fn rename_entry_path(source: PathBuf, requested_name: &str) -> Result<PathBuf, FsError> {
    let name = requested_name.trim();
    let name_path = Path::new(name);
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 255
        || name.contains(['/', '\\', '\0'])
        || name_path.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(FsError::InvalidFileName(requested_name.to_owned()));
    }

    let parent = source
        .parent()
        .ok_or_else(|| FsError::PathNotAbsolute(display_path(&source)))?;
    let destination = parent.join(name);
    if destination == source {
        return Ok(destination);
    }

    rename_no_replace(&source, &destination)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                FsError::AlreadyExists(display_path(&destination))
            } else {
                FsError::Io(error)
            }
        })?;
    Ok(destination)
}

#[cfg(target_os = "linux")]
async fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    // musl toolchains do not consistently export a renameat2 wrapper. Use the
    // Linux syscall directly to retain atomic no-replace behavior with either libc.
    // SAFETY: Both pointers are valid NUL-terminated paths for the duration of the call.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
async fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    // Darwin handles case-only renames of the same directory entry while
    // atomically rejecting other existing entries, including hard links.
    // Do not fall back to check-then-rename on volumes without RENAME_EXCL.
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
async fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(destination).await {
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "destination already exists",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(source, destination).await
}
