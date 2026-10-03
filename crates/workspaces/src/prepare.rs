use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::{WorkspaceConfig, WorkspaceMode};

/// Hosts supply Git execution so their environment, cancellation and process
/// ownership tracking also apply to workspace preparation.
pub trait GitExecutor: Sync {
    fn output(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> impl std::future::Future<Output = Result<String>> + Send;
}

/// A prepared directory has no automatic cleanup on drop.
#[derive(Clone, Debug)]
pub struct PreparedWorkspace {
    pub directory: PathBuf,
    pub workspace_mode: WorkspaceMode,
    pub branch: Option<String>,
    pub created: bool,
}

struct Worktree {
    path: PathBuf,
    branch: Option<String>,
}

async fn worktrees(repository: &Path, git: &impl GitExecutor) -> Result<Vec<Worktree>> {
    let output = git
        .output(repository, &["worktree", "list", "--porcelain", "-z"])
        .await?;
    Ok(output
        .split("\0\0")
        .filter_map(|record| {
            let fields: Vec<_> = record.split('\0').collect();
            let path = fields
                .iter()
                .find_map(|field| field.strip_prefix("worktree "))?;
            let branch = fields
                .iter()
                .find_map(|field| field.strip_prefix("branch refs/heads/"));
            Some(Worktree {
                path: path.into(),
                branch: branch.map(str::to_owned),
            })
        })
        .collect())
}

fn sibling_directory(main: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty()
            && name.len() <= 512
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_/".contains(c)),
        "工作区名称无效"
    );
    let parent = main.parent().context("主仓库没有可用的父目录")?;
    Ok(parent.join(format!(".aow-{}", name.replace('/', "-"))))
}

pub async fn worktree_directory(
    repository: &Path,
    name: &str,
    git: &impl GitExecutor,
) -> Result<PathBuf> {
    let trees = worktrees(repository, git).await?;
    let main = trees.first().context("项目没有可用的 Worktree")?;
    sibling_directory(&main.path, name)
}

pub async fn prepare(
    config: &WorkspaceConfig,
    repository: &Path,
    name: &str,
    git: &impl GitExecutor,
) -> Result<PreparedWorkspace> {
    config.validate()?;
    if config.workspace_mode == WorkspaceMode::Temporary {
        ensure!(
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_/".contains(c)),
            "工作区名称无效"
        );
        let temporary = tempfile::Builder::new()
            .prefix(&format!("aow-{}-", name.replace('/', "-")))
            .tempdir()
            .context("无法创建临时工作区")?;
        let directory = temporary.path().canonicalize()?;
        let _ = temporary.keep();
        return Ok(PreparedWorkspace {
            directory,
            workspace_mode: config.workspace_mode,
            branch: None,
            created: true,
        });
    }
    let repository = repository.canonicalize().context("项目目录不存在")?;
    let trees = worktrees(&repository, git).await?;
    let main = trees.first().context("项目没有可用的 Worktree")?;
    if config.workspace_mode == WorkspaceMode::Existing {
        let directory = config
            .workspace_path
            .canonicalize()
            .context("工作区目录不存在")?;
        // Automation hosting can execute inside a Worktree subdirectory. Git
        // ownership also excludes nested repositories and submodules.
        let worktree_root = if trees.iter().any(|tree| tree.path == directory) {
            directory.clone()
        } else {
            PathBuf::from(
                git.output(&directory, &["rev-parse", "--show-toplevel"])
                    .await?
                    .trim_end_matches(['\r', '\n']),
            )
            .canonicalize()?
        };
        let selected = trees
            .iter()
            .find(|tree| tree.path == worktree_root)
            .context("工作区不属于所选项目")?;
        return Ok(PreparedWorkspace {
            directory,
            workspace_mode: config.workspace_mode,
            branch: selected.branch.clone(),
            created: false,
        });
    }
    let directory = sibling_directory(&main.path, name)?;
    if let Some(existing) = trees
        .iter()
        .find(|tree| tree.branch.as_deref() == Some(name))
    {
        ensure!(
            existing.path == directory,
            "本次执行的分支已被其他 Worktree 使用"
        );
        return Ok(PreparedWorkspace {
            directory,
            workspace_mode: config.workspace_mode,
            branch: Some(name.into()),
            created: false,
        });
    }
    git.output(&main.path, &["check-ref-format", "--branch", name])
        .await?;
    let base = git
        .output(
            &main.path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{}^{{commit}}", config.base_branch.trim()),
            ],
        )
        .await?;
    git.output(
        &main.path,
        &[
            "worktree",
            "add",
            "-b",
            name,
            "--",
            directory.to_str().context("工作区路径不是 UTF-8")?,
            base.trim(),
        ],
    )
    .await?;
    Ok(PreparedWorkspace {
        directory: directory.canonicalize()?,
        workspace_mode: config.workspace_mode,
        branch: Some(name.into()),
        created: true,
    })
}
