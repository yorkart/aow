//! Release resources only seed/update files; managed-directory discovery loads them.
use super::{GitPlatformProvider, defaults};
#[cfg(debug_assertions)]
use std::path::PathBuf;

pub(super) fn load(
    node: Option<&aow_runtime_client::RuntimeClient>,
) -> anyhow::Result<Vec<GitPlatformProvider>> {
    if let Some(root) = std::env::current_exe()?
        .parent()
        .and_then(|path| path.parent())
    {
        let directory = root.join("git-platform-providers");
        if directory.is_dir() {
            return defaults::load_with_runtime(&directory, node);
        }
    }
    #[cfg(debug_assertions)]
    {
        let directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../git-platform-providers");
        if directory.is_dir() {
            return defaults::load_with_runtime(&directory, node);
        }
    }
    // Existing configurations remain usable without an adjacent release bundle.
    Ok(Vec::new())
}

pub(crate) fn seed_configuration(state_dir: &std::path::Path) -> anyhow::Result<()> {
    let config = aow_config::ConfigStore::initialize(state_dir)?;
    let mut directories = Vec::new();
    if let Some(root) = std::env::current_exe()?
        .parent()
        .and_then(|path| path.parent())
    {
        directories.push(root.join("git-platform-providers"));
    }
    #[cfg(debug_assertions)]
    directories
        .push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../git-platform-providers"));
    if let Some(directory) = directories.into_iter().find(|directory| directory.is_dir()) {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !super::validation::valid_filename(name) {
                continue;
            }
            let destination = std::path::Path::new(super::settings::DIRECTORY).join(name);
            if !config.directory().join(&destination).exists() {
                config.save(&destination, defaults::read_script(&path)?.as_bytes())?;
            }
        }
    }
    Ok(())
}
