//! Release resources only seed/update files; managed-directory discovery loads them.
use super::{GitPlatformProvider, defaults};
#[cfg(debug_assertions)]
use std::path::PathBuf;

pub(super) fn load() -> anyhow::Result<Vec<GitPlatformProvider>> {
    if let Some(root) = std::env::current_exe()?
        .parent()
        .and_then(|path| path.parent())
    {
        let directory = root.join("git-platform-providers");
        if directory.is_dir() {
            return defaults::load(&directory);
        }
    }
    #[cfg(debug_assertions)]
    {
        let directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../git-platform-providers");
        if directory.is_dir() {
            return defaults::load(&directory);
        }
    }
    // Existing configurations remain usable without an adjacent release bundle.
    Ok(Vec::new())
}
