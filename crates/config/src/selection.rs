use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, value};

use super::{
    ConfigRepository,
    git::git_text,
    layout::REGISTRIES,
    storage::{FileLock, atomic_write, private_directories},
};
pub const CONFIG_FILE: &str = "config.toml";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigSelection {
    pub config_repo: PathBuf,
    pub config_id: String,
}

#[derive(Debug, Serialize)]
pub struct RepositoryVersions {
    pub config_repo: PathBuf,
    pub config_ids: Vec<String>,
    pub selected_id: Option<String>,
}

/// Load once at process startup. Reads and saves use this in-memory document;
/// external edits only take effect when the process starts again.
pub struct ConfigFile {
    state_dir: PathBuf,
    document: DocumentMut,
    selection: ConfigSelection,
}

impl ConfigFile {
    pub fn load(state_dir: &Path) -> Result<Self> {
        let document = read_document(state_dir)?
            .context(aow_i18n::text("config.config_toml_does_not_exist"))?;
        let selection =
            ConfigRepository::from_selection(&document_selection(&document)?)?.selection();
        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            document,
            selection,
        })
    }

    /// The saved selection for the next startup, not the currently active repository.
    pub fn selection(&self) -> &ConfigSelection {
        &self.selection
    }

    pub fn save(&mut self, selection: &ConfigSelection) -> Result<bool> {
        let selection = ConfigRepository::from_selection(selection)?.selection();
        if selection == self.selection {
            return Ok(false);
        }
        let mut document = self.document.clone();
        update_document(&mut document, &selection)?;
        let _lock = FileLock::acquire(&self.state_dir.join(".config-init.lock"))?;
        atomic_write(
            &self.state_dir.join(CONFIG_FILE),
            document.to_string().as_bytes(),
        )?;
        // Publish the saved selection only after the write succeeds. Active
        // ConfigRepository handles are never replaced by a Settings save.
        self.document = document;
        self.selection = selection;
        Ok(true)
    }
}

impl ConfigRepository {
    pub fn selection(&self) -> ConfigSelection {
        ConfigSelection {
            config_repo: self.repository.clone(),
            config_id: self
                .directory
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned(),
        }
    }

    pub fn from_selection(selection: &ConfigSelection) -> Result<Self> {
        ensure!(
            selection.config_repo.is_absolute(),
            aow_i18n::message("config.config_repo_must_be_an_absolute_path", &[])
        );
        ensure!(
            valid_config_id(&selection.config_id),
            aow_i18n::message("config.invalid_configuration_id", &[])
        );
        let repository = repository_root(&selection.config_repo)?;
        let config = Self::from_directory(&repository.join(&selection.config_id))?;
        ensure!(
            config.repository == repository && config.selection().config_id == selection.config_id,
            aow_i18n::message(
                "config.the_configuration_version_must_be_a_version_directory_at",
                &[]
            )
        );
        Ok(config)
    }
}

pub(super) fn repository_root(path: &Path) -> Result<PathBuf> {
    let repository = fs::canonicalize(path).with_context(|| {
        aow_i18n::message(
            "config.repository_directory_missing_or_inaccessible",
            &[("auto0", format!("{}", path.display()))],
        )
    })?;
    ensure!(
        repository.is_dir(),
        aow_i18n::message(
            "config.repository_path_is_not_a_directory",
            &[("auto0", format!("{}", path.display()))]
        )
    );
    let top = git_text(&repository, &["rev-parse", "--show-toplevel"]).with_context(|| {
        aow_i18n::message(
            "config.path_is_not_a_git_repository",
            &[("auto0", format!("{}", path.display()))],
        )
    })?;
    ensure!(
        fs::canonicalize(top)? == repository,
        aow_i18n::message(
            "config.path_must_be_the_git_repository_root",
            &[("auto0", format!("{}", path.display()))]
        )
    );
    Ok(repository)
}

/// List configuration version directories, accepting a repository or one of its versions.
pub fn inspect_repository(path: &Path) -> Result<RepositoryVersions> {
    ensure!(
        path.is_absolute(),
        aow_i18n::message("config.enter_the_absolute_path_of_a_git_repository", &[])
    );
    let directory = fs::canonicalize(path).with_context(|| {
        aow_i18n::message(
            "config.directory_missing_or_inaccessible",
            &[("auto0", format!("{}", path.display()))],
        )
    })?;
    ensure!(
        directory.is_dir(),
        aow_i18n::message(
            "config.path_is_not_a_directory",
            &[("auto0", format!("{}", path.display()))]
        )
    );
    let version = directory
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| valid_config_id(name) && has_configuration(&directory));
    let parent_repository = version
        .and_then(|_| directory.parent())
        .and_then(|parent| repository_root(parent).ok());
    let (repository, selected_id) = if let Some(repository) = parent_repository {
        (repository, version.map(str::to_owned))
    } else {
        (repository_root(&directory)?, None)
    };
    let mut config_ids = Vec::new();
    for entry in fs::read_dir(&repository)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(name) = entry.file_name().to_str()
            && valid_config_id(name)
            && has_configuration(&entry.path())
        {
            config_ids.push(name.to_owned());
        }
    }
    config_ids.sort();
    Ok(RepositoryVersions {
        config_repo: repository,
        config_ids,
        selected_id,
    })
}

fn read_document(state_dir: &Path) -> Result<Option<DocumentMut>> {
    let path = state_dir.join(CONFIG_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                !path.is_symlink(),
                aow_i18n::message("config.config_toml_is_a_broken_symbolic_link", &[])
            );
            return Ok(None);
        }
        Err(error) => {
            return Err(error).context(aow_i18n::text("config.unable_to_read_config_toml"));
        }
    };
    Ok(Some(text.parse().context(aow_i18n::text(
        "config.invalid_config_toml_format",
    ))?))
}

fn document_selection(document: &DocumentMut) -> Result<ConfigSelection> {
    let config_repo = document
        .get("config-repo")
        .and_then(|item| item.as_str())
        .filter(|path| !path.is_empty())
        .context(aow_i18n::text(
            "config.config_toml_is_missing_string_field_config_repo",
        ))?;
    let config_id = document
        .get("config-id")
        .and_then(|item| item.as_str())
        .context(aow_i18n::text(
            "config.config_toml_is_missing_string_field_config_id",
        ))?;
    Ok(ConfigSelection {
        config_repo: PathBuf::from(config_repo),
        config_id: config_id.to_owned(),
    })
}

pub(super) fn read_selection(state_dir: &Path) -> Result<Option<ConfigSelection>> {
    read_document(state_dir)?
        .as_ref()
        .map(document_selection)
        .transpose()
}

fn update_document(document: &mut DocumentMut, selection: &ConfigSelection) -> Result<()> {
    document["config-repo"] = value(
        selection
            .config_repo
            .to_str()
            .context(aow_i18n::text("config.configuration_path_must_be_utf_8"))?,
    );
    document["config-id"] = value(&selection.config_id);
    Ok(())
}

/// Caller holds the state-directory lock. Preserve other settings and TOML comments.
pub(super) fn write_selection(state_dir: &Path, selection: &ConfigSelection) -> Result<bool> {
    let previous = read_document(state_dir)?;
    let changed = previous
        .as_ref()
        .map(document_selection)
        .transpose()?
        .as_ref()
        != Some(selection);
    if changed {
        let mut document = previous.unwrap_or_default();
        update_document(&mut document, selection)?;
        atomic_write(
            &state_dir.join(CONFIG_FILE),
            document.to_string().as_bytes(),
        )?;
    }
    Ok(changed)
}

/// Persist the next startup's selection; existing ConfigRepository handles stay active.
/// Returns the normalized selection and whether the saved selection changed.
pub fn save_selection(
    state_dir: &Path,
    selection: &ConfigSelection,
) -> Result<(ConfigSelection, bool)> {
    let config = ConfigRepository::from_selection(selection)?;
    let selection = config.selection();
    private_directories(state_dir)?;
    let _lock = FileLock::acquire(&state_dir.join(".config-init.lock"))?;
    let changed = write_selection(state_dir, &selection)?;
    Ok((selection, changed))
}

/// IDs are opaque path components; configuration contents identify a version.
pub(super) fn valid_config_id(id: &str) -> bool {
    aow_id::is_valid_id(id)
}

fn has_configuration(directory: &Path) -> bool {
    REGISTRIES.iter().any(|name| directory.join(name).is_file())
}
