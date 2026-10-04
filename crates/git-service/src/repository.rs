use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use walkdir::{DirEntry, WalkDir};

use aow_protocol::RepositorySummary;

use super::{
    GitError,
    command::{absolute, run_git},
};

pub async fn discover_repositories(
    root: impl AsRef<Path>,
    maximum_depth: usize,
) -> Result<Vec<RepositorySummary>, GitError> {
    let root = absolute(root)?;
    let roots = tokio::task::spawn_blocking(move || discover_sync(&root, maximum_depth))
        .await
        .map_err(|error| GitError::Command(error.to_string()))??;
    let mut repositories = Vec::with_capacity(roots.len());
    for path in roots {
        let branch = current_branch(&path)
            .await
            .unwrap_or_else(|_| "HEAD".to_owned());
        repositories.push(RepositorySummary {
            name: path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            path: path.to_string_lossy().into_owned(),
            branch,
        });
    }
    repositories.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(repositories)
}

pub(super) async fn repository_root_for_path(path: &Path) -> Result<PathBuf, GitError> {
    let output = match run_git(path, &["rev-parse", "--show-toplevel"], 64 * 1024).await {
        Ok(output) => output,
        Err(GitError::Command(_)) => {
            return Err(GitError::NotRepository(path.to_string_lossy().into_owned()));
        }
        Err(error) => return Err(error),
    };
    if output.truncated {
        return Err(GitError::Command(
            "git repository path output exceeded limit".to_owned(),
        ));
    }
    let repository = String::from_utf8(output.bytes)
        .map_err(|_| GitError::InvalidUtf8)?
        .trim()
        .to_owned();
    if repository.is_empty() {
        return Err(GitError::NotRepository(path.to_string_lossy().into_owned()));
    }
    let repository = PathBuf::from(repository);
    if !repository.is_absolute() {
        return Err(GitError::Command(
            "git returned a non-absolute repository path".to_owned(),
        ));
    }
    Ok(repository)
}
fn discover_sync(root: &Path, maximum_depth: usize) -> Result<Vec<PathBuf>, GitError> {
    let mut repositories = BTreeSet::new();
    for entry in WalkDir::new(root)
        .max_depth(maximum_depth.saturating_add(1))
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
    {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_dir() && entry.path().join(".git").exists() {
            repositories.insert(entry.path().to_path_buf());
        }
    }
    Ok(repositories.into_iter().collect())
}

fn should_descend(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git" | "node_modules" | "target" | "dist" | "build"
    )
}

pub(super) async fn current_branch(repository: &Path) -> Result<String, GitError> {
    let output = run_git(
        repository,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        64 * 1024,
    )
    .await;
    match output {
        Ok(output) => Ok(String::from_utf8_lossy(&output.bytes).trim().to_owned()),
        Err(_) => {
            let output = run_git(repository, &["rev-parse", "--short", "HEAD"], 64 * 1024).await?;
            Ok(format!(
                "detached@{}",
                String::from_utf8_lossy(&output.bytes).trim()
            ))
        }
    }
}
