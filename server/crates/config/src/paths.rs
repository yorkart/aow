use std::path::PathBuf;

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
