use std::{io::Write, path::Path};

use chrono::{SecondsFormat, Utc};

use super::{METADATA_FILE, PersistedState, TerminalError};

pub(super) fn atomic_save(path: &Path, state: &PersistedState) -> Result<(), TerminalError> {
    let parent = path.parent().ok_or_else(|| {
        TerminalError::Invalid(format!("metadata path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)?;
    let temp_path = parent.join(format!(".{METADATA_FILE}.{}.tmp", aow_id::new_id()));
    let result = (|| {
        let bytes = serde_json::to_vec_pretty(state)?;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)?;
        Ok::<_, TerminalError>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

pub(super) fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
