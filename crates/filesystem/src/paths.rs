use std::path::{Path, PathBuf};

use super::FsError;

/// Default local AoW state, shared by the server and command-line tools.
pub fn default_state_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("AOW_STATE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("aow");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".local").join("state").join("aow");
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
