use std::path::{Path, PathBuf};

use anyhow::Result;

use super::ConfigRepository;

/// Read-only clients support older state directories until a writer migrates them.
pub fn configuration_directory(state_dir: &Path) -> Result<PathBuf> {
    Ok(ConfigRepository::open(state_dir)?
        .map(|config| config.directory().to_path_buf())
        .unwrap_or_else(|| state_dir.to_path_buf()))
}
