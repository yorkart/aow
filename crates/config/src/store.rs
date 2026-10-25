use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{
    layout::{REGISTRIES, is_configuration},
    storage::{FileLock, atomic_write, private_directories},
};

#[derive(Clone, Debug)]
pub struct ConfigStore {
    directory: PathBuf,
}

impl ConfigStore {
    /// Resolve the application home without creating or changing any files.
    pub fn open(home: &Path) -> Result<Self> {
        let directory = if home.is_absolute() {
            home.to_path_buf()
        } else {
            std::env::current_dir()?.join(home)
        };
        Ok(Self { directory })
    }

    pub fn initialize(home: &Path) -> Result<Self> {
        private_directories(home)?;
        let config = Self {
            directory: fs::canonicalize(home)?,
        };
        let _lock = config.lock()?;
        for name in REGISTRIES {
            let path = config.directory.join(name);
            if name != "aow-settings.json" && !path.try_exists()? {
                atomic_write(&path, b"{\n  \"version\": 1,\n  \"items\": []\n}\n")?;
            }
        }
        Ok(config)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn save(&self, relative: &Path, bytes: &[u8]) -> Result<()> {
        ensure!(
            is_configuration(relative),
            aow_i18n::message(
                "config.unsupported_configuration_file",
                &[("auto0", relative.display().to_string())]
            )
        );
        let _lock = self.lock()?;
        let parent = relative.parent().context(aow_i18n::text(
            "config.configuration_file_has_no_parent_directory",
        ))?;
        self.prepare_directory(parent)?;
        let path = self.directory.join(relative);
        ensure!(
            !path.is_symlink(),
            aow_i18n::message(
                "config.configuration_file_cannot_be_a_symbolic_link",
                &[("auto0", path.display().to_string())]
            )
        );
        match fs::read(&path) {
            Ok(previous) if previous == bytes => Ok(()),
            Ok(_) => atomic_write(&path, bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                atomic_write(&path, bytes)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Serialize a group of durable writes across processes.
    pub fn update_directory(
        &self,
        relative: &Path,
        update: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            matches!(
                relative.to_str(),
                Some("data/inbox" | "automations/tasks" | "git-platform-providers")
            ),
            aow_i18n::message(
                "config.unsupported_configuration_directory",
                &[("auto0", relative.display().to_string())]
            )
        );
        let _lock = self.lock()?;
        let directory = self.prepare_directory(relative)?;
        check_directory(&directory)?;
        update(&directory)
    }

    fn prepare_directory(&self, relative: &Path) -> Result<PathBuf> {
        let mut directory = self.directory.clone();
        for component in relative.components() {
            directory.push(component);
            ensure!(
                !directory.is_symlink(),
                aow_i18n::message(
                    "config.configuration_directory_cannot_be_a_symbolic_link",
                    &[]
                )
            );
            private_directories(&directory)?;
        }
        Ok(directory)
    }

    fn lock(&self) -> Result<FileLock> {
        let directory = self.prepare_directory(Path::new("state"))?;
        FileLock::acquire(&directory.join("config.lock"))
    }
}

fn check_directory(directory: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            aow_i18n::message(
                "config.configuration_directory_cannot_contain_symbolic_links",
                &[("auto0", entry.path().display().to_string())]
            )
        );
        if kind.is_dir() {
            check_directory(&entry.path())?;
        } else {
            ensure!(
                kind.is_file(),
                aow_i18n::message(
                    "config.configuration_must_be_a_regular_file",
                    &[("auto0", entry.path().display().to_string())]
                )
            );
        }
    }
    Ok(())
}
