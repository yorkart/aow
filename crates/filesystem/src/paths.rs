use std::path::{Path, PathBuf};

use super::FsError;

/// Application home containing configuration, data, state, cache, and runtime files.
pub fn default_state_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("AOW_STATE_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".aow");
    }
    std::env::temp_dir().join(format!("aow-{}", std::process::id()))
}
pub fn absolute_path(raw: impl AsRef<Path>) -> Result<PathBuf, FsError> {
    let path = raw.as_ref();
    if !path.is_absolute() {
        return Err(FsError::PathNotAbsolute(
            path.to_string_lossy().into_owned(),
        ));
    }
    Ok(path.to_path_buf())
}
pub(super) fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Socket shared by the terminal daemon and its clients.
pub fn default_terminald_socket() -> PathBuf {
    std::env::var_os("AOW_TERMINALD_SOCKET")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_state_dir().join("runtime/terminald/terminald.sock"))
}
