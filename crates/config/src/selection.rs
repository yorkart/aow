use super::*;
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, value};

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
        let document = read_document(state_dir)?.context("config.toml 不存在")?;
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
            "config-repo 必须是绝对路径"
        );
        ensure!(
            Uuid::parse_str(&selection.config_id).is_ok(),
            "config-id 必须是 UUID"
        );
        let repository = repository_root(&selection.config_repo)?;
        let config = Self::from_directory(&repository.join(&selection.config_id))?;
        ensure!(
            config.repository == repository && config.selection().config_id == selection.config_id,
            "配置版本必须是该 Git 仓库根目录下对应的 UUID 目录"
        );
        Ok(config)
    }
}

pub(super) fn repository_root(path: &Path) -> Result<PathBuf> {
    let repository = fs::canonicalize(path)
        .with_context(|| format!("仓库目录不存在或无法访问：{}", path.display()))?;
    ensure!(repository.is_dir(), "仓库路径不是目录：{}", path.display());
    let top = git_text(&repository, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("路径不是 Git 仓库：{}", path.display()))?;
    ensure!(
        fs::canonicalize(top)? == repository,
        "路径必须是 Git 仓库根目录：{}",
        path.display()
    );
    Ok(repository)
}

/// List UUID directories, accepting either a repository or one of its versions.
pub fn inspect_repository(path: &Path) -> Result<RepositoryVersions> {
    ensure!(path.is_absolute(), "请输入 Git 仓库的绝对路径");
    let directory = fs::canonicalize(path)
        .with_context(|| format!("目录不存在或无法访问：{}", path.display()))?;
    ensure!(directory.is_dir(), "路径不是目录：{}", path.display());
    let version = directory
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| Uuid::parse_str(name).is_ok());
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
            && Uuid::parse_str(name).is_ok()
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
            ensure!(!path.is_symlink(), "config.toml 是失效的符号链接");
            return Ok(None);
        }
        Err(error) => return Err(error).context("无法读取 config.toml"),
    };
    Ok(Some(text.parse().context("config.toml 格式无效")?))
}

fn document_selection(document: &DocumentMut) -> Result<ConfigSelection> {
    let config_repo = document
        .get("config-repo")
        .and_then(|item| item.as_str())
        .filter(|path| !path.is_empty())
        .context("config.toml 缺少字符串字段 config-repo")?;
    let config_id = document
        .get("config-id")
        .and_then(|item| item.as_str())
        .context("config.toml 缺少字符串字段 config-id")?;
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
            .context("配置路径必须是 UTF-8")?,
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
