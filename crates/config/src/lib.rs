//! Machine configuration lives in a Git repository; runtime state stays local.
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use anyhow::{Context, Result, ensure};
use uuid::Uuid;

mod selection;
pub use selection::{
    CONFIG_FILE, ConfigFile, ConfigSelection, RepositoryVersions, inspect_repository,
    save_selection,
};

pub const REPOSITORY_DIRECTORY: &str = "config-repo";
const REGISTRIES: [&str; 3] = ["aow-projects.json", "aow-agents.json", "aow-settings.json"];

#[derive(Clone, Debug)]
pub struct ConfigRepository {
    directory: PathBuf,
    repository: PathBuf,
    git_directory: PathBuf,
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

    fn from_directory(directory: &Path) -> Result<Self> {
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

/// Read-only clients support older state directories until a writer migrates them.
pub fn configuration_directory(state_dir: &Path) -> Result<PathBuf> {
    Ok(ConfigRepository::open(state_dir)?
        .map(|config| config.directory)
        .unwrap_or_else(|| state_dir.to_path_buf()))
}

fn is_configuration(path: &Path) -> bool {
    if REGISTRIES.iter().any(|name| path == Path::new(name))
        || path == Path::new("review-providers.json")
    {
        return true;
    }
    ((path.parent() == Some(Path::new("automations/tasks"))
        && path.extension().is_some_and(|ext| ext == "json"))
        || (path.parent() == Some(Path::new("review-providers"))
            && path.extension().is_some_and(|ext| ext == "py")))
        && path.file_stem().and_then(OsStr::to_str).is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 96
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
}

fn copy_json(source: &Path, destination: &Path) -> Result<(PathBuf, Vec<u8>)> {
    ensure!(
        fs::symlink_metadata(source)?.is_file(),
        "配置必须是普通文件：{}",
        source.display()
    );
    let bytes = fs::read(source)?;
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .with_context(|| format!("原配置 JSON 无效：{}", source.display()))?;
    atomic_write(destination, &bytes)?;
    Ok((source.to_path_buf(), bytes))
}

fn git_command(repository: &Path) -> Command {
    let mut command = Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(name);
    }
    command
        .current_dir(repository)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .args([
            "-c",
            "user.name=AoW",
            "-c",
            "user.email=aow@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .stdin(Stdio::null());
    command
}

fn git_checked(command: &mut Command) -> Result<Output> {
    let output = command.output().context("无法执行配置仓库 Git 命令")?;
    ensure!(
        output.status.success(),
        "配置仓库 Git 操作失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

fn git_text(repository: &Path, args: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(git_checked(git_command(repository).args(args))?.stdout)?
            .trim_end_matches(['\r', '\n'])
            .to_owned(),
    )
}

struct FileLock(File);

impl FileLock {
    fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Ok(Self(file));
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("配置文件缺少父目录")?;
    private_directories(parent)?;
    let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn private_directories(path: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}
