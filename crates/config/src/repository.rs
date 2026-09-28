use std::{
    ffi::OsStr,
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use uuid::Uuid;

use super::{
    git::{git_checked, git_command, git_text},
    layout::{REGISTRIES, REPOSITORY_DIRECTORY, is_configuration},
    selection,
    storage::{FileLock, atomic_write, copy_json, private_directories},
};

#[derive(Clone, Debug)]
pub struct ConfigRepository {
    pub(super) directory: PathBuf,
    pub(super) repository: PathBuf,
    pub(super) git_directory: PathBuf,
}

impl ConfigRepository {
    /// Resolve an existing selection without initializing or writing anything.
    /// A missing config.toml means no selection; an invalid file is an error.
    pub fn open(state_dir: &Path) -> Result<Option<Self>> {
        selection::read_selection(state_dir)?
            .as_ref()
            .map(Self::from_selection)
            .transpose()
    }

    pub(super) fn from_directory(directory: &Path) -> Result<Self> {
        let directory = fs::canonicalize(directory)
            .with_context(|| format!("配置目录不存在或无法访问：{}", directory.display()))?;
        ensure!(
            directory.is_dir(),
            "配置路径不是目录：{}",
            directory.display()
        );
        let name = directory
            .file_name()
            .and_then(OsStr::to_str)
            .context("配置目录名必须是 UUID")?;
        ensure!(
            Uuid::parse_str(name).is_ok(),
            "配置目录名必须是 UUID：{name}"
        );
        let repository = directory
            .parent()
            .context("配置目录缺少仓库父目录")?
            .to_path_buf();
        selection::repository_root(&repository)?;
        // Resolve relative output ourselves; --path-format requires Git 2.31.
        let common = git_text(&repository, &["rev-parse", "--git-common-dir"])?;
        let git_directory = fs::canonicalize(repository.join(common))?;
        Ok(Self {
            directory,
            repository,
            git_directory,
        })
    }

    /// Initialize a local repository and copy only configuration from older installs.
    /// Original configuration stays in place. Publish config.toml after the initial commit.
    pub fn initialize(state_dir: &Path) -> Result<Self> {
        if let Some(config) = Self::open(state_dir)? {
            return Ok(config);
        }
        private_directories(state_dir)?;
        let state_dir = fs::canonicalize(state_dir)?;
        let _initialization = FileLock::acquire(&state_dir.join(".config-init.lock"))?;
        if let Some(config) = Self::open(&state_dir)? {
            return Ok(config);
        }
        let repository = state_dir.join(REPOSITORY_DIRECTORY);
        private_directories(&repository)?;
        if !repository.join(".git").exists() {
            ensure!(
                fs::read_dir(&repository)?.next().is_none(),
                "配置仓库目录非空且不是 Git 仓库：{}",
                repository.display()
            );
            // --initial-branch requires Git 2.28. Set HEAD before the first commit
            // using commands that also work on older Git versions.
            git_text(&repository, &["init", "--template="])?;
            git_text(&repository, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
        }
        let directory = repository.join(Uuid::new_v4().to_string());
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let config = Self::from_directory(&directory)?;
        let _save = config.lock()?;
        let mut copied = Vec::new();
        for name in REGISTRIES {
            let source = state_dir.join(name);
            if source.try_exists()? {
                copied.push(copy_json(&source, &directory.join(name))?);
            } else if name != "aow-settings.json" {
                atomic_write(
                    &directory.join(name),
                    b"{\n  \"version\": 1,\n  \"items\": []\n}\n",
                )?;
            }
        }
        let tasks = state_dir.join("automations/tasks");
        if tasks.try_exists()? {
            for entry in fs::read_dir(tasks)? {
                let entry = entry?;
                if entry.path().extension().is_some_and(|ext| ext == "json") {
                    copied.push(copy_json(
                        &entry.path(),
                        &directory.join("automations/tasks").join(entry.file_name()),
                    )?);
                }
            }
        }
        config.commit(&directory, "Initialize machine configuration")?;
        // An older running service does not take our lock. Do not publish a
        // snapshot if a source changed while we were copying and committing it.
        for (source, bytes) in copied {
            ensure!(
                fs::read(&source)? == bytes,
                "初始化期间原配置发生变化，请重试：{}",
                source.display()
            );
        }
        selection::write_selection(&state_dir, &config.selection())?;
        Ok(config)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Serialize every save and commit across processes using this repository.
    /// A failed commit leaves the saved file intact and is reported to the caller;
    /// saving it again retries the commit, even if the bytes have not changed.
    pub fn save(&self, relative: &Path, bytes: &[u8]) -> Result<()> {
        ensure!(
            is_configuration(relative),
            "不支持的配置文件：{}",
            relative.display()
        );
        let _save = self.lock()?;
        let path = self.directory.join(relative);
        let mut parent = self.directory.clone();
        for component in relative
            .parent()
            .context("配置文件缺少父目录")?
            .components()
        {
            parent.push(component);
            ensure!(!parent.is_symlink(), "配置文件不能通过符号链接写入其他目录");
            private_directories(&parent)?;
        }
        ensure!(
            !path.is_symlink(),
            "配置文件不能是符号链接：{}",
            path.display()
        );
        match fs::read(&path) {
            Ok(previous) if previous == bytes => {}
            Ok(_) => atomic_write(&path, bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                atomic_write(&path, bytes)?
            }
            Err(error) => return Err(error.into()),
        }
        self.commit(
            &path,
            &format!(
                "Save {}: {}",
                self.directory.file_name().unwrap().to_string_lossy(),
                relative.display()
            ),
        )
        .context("配置文件已保存，但 Git 提交失败；再次保存可重试")
    }

    fn lock(&self) -> Result<FileLock> {
        FileLock::acquire(&self.git_directory.join("aow-config.lock"))
    }

    fn commit(&self, path: &Path, message: &str) -> Result<()> {
        let relative = path.strip_prefix(&self.repository)?;
        git_checked(
            git_command(&self.repository)
                .args(["add", "--all", "--"])
                .arg(relative),
        )?;
        let diff = git_command(&self.repository)
            .args(["diff", "--cached", "--quiet", "--"])
            .arg(relative)
            .output()?;
        if diff.status.success() {
            return Ok(());
        }
        ensure!(
            diff.status.code() == Some(1),
            "无法检查配置 Git 变更：{}",
            String::from_utf8_lossy(&diff.stderr)
        );
        // --only preserves unrelated staged changes, including other machines.
        git_checked(
            git_command(&self.repository)
                .args(["commit", "--only", "-m", message, "--"])
                .arg(relative),
        )?;
        Ok(())
    }
}
