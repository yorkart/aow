use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use aow_protocol::{GitFileStatus, GitIgnoredPaths, GitStatus};

use super::{
    GitError,
    command::{
        LimitedOutput, SMALL_OUTPUT_LIMIT, absolute, run_git, run_git_check_ignore,
        run_git_with_timeout,
    },
    repository::{current_branch, repository_root_for_path},
};

pub async fn ignored_paths(
    root: impl AsRef<Path>,
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<GitIgnoredPaths, GitError> {
    let root = absolute(root)?;
    let repository = match repository_root_for_path(&root).await {
        Ok(repository) => repository,
        Err(GitError::NotRepository(_)) => {
            return Ok(GitIgnoredPaths {
                repository: None,
                ignored: Vec::new(),
            });
        }
        Err(error) => return Err(error),
    };

    let mut input = Vec::new();
    let mut candidates = std::collections::HashMap::new();
    for path in paths {
        let path = absolute(path)?;
        let Ok(relative) = path.strip_prefix(&repository) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let relative = relative.to_string_lossy().into_owned();
        input.extend_from_slice(relative.as_bytes());
        input.push(0);
        candidates.insert(relative, path.to_string_lossy().into_owned());
    }

    let mut ignored = Vec::new();
    if !input.is_empty() {
        let output = run_git_check_ignore(&repository, input).await?;
        if output.truncated {
            return Err(GitError::Command(
                "git ignored-path output exceeded limit".to_owned(),
            ));
        }
        for record in output.bytes.split(|byte| *byte == 0) {
            if record.is_empty() {
                continue;
            }
            let relative = String::from_utf8_lossy(record);
            if let Some(path) = candidates.get(relative.as_ref()) {
                ignored.push(path.clone());
            }
        }
    }

    Ok(GitIgnoredPaths {
        repository: Some(repository.to_string_lossy().into_owned()),
        ignored,
    })
}

pub async fn status(repository: impl AsRef<Path>) -> Result<GitStatus, GitError> {
    let repository = absolute(repository)?;
    let branch = current_branch(&repository).await?;
    let output = run_git(
        &repository,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        SMALL_OUTPUT_LIMIT,
    )
    .await?;
    let files = parse_status(&output);
    let (ahead, behind) = ahead_behind(&repository).await.unwrap_or((0, 0));
    Ok(GitStatus {
        repository: repository.to_string_lossy().into_owned(),
        branch,
        ahead,
        behind,
        files,
    })
}

pub async fn pull(repository: impl AsRef<Path>) -> Result<(), GitError> {
    sync_remote(repository, "pull").await
}

pub async fn push(repository: impl AsRef<Path>) -> Result<(), GitError> {
    sync_remote(repository, "push").await
}

async fn sync_remote(repository: impl AsRef<Path>, command: &str) -> Result<(), GitError> {
    let repository = absolute(repository)?;
    let output = run_git_with_timeout(
        &repository,
        &[command],
        SMALL_OUTPUT_LIMIT,
        &[("GIT_TERMINAL_PROMPT", "0"), ("GIT_EDITOR", "true")],
        Duration::from_secs(120),
    )
    .await?;
    if output.truncated {
        return Err(GitError::Command(format!(
            "git {command} output exceeded limit"
        )));
    }
    Ok(())
}
async fn ahead_behind(repository: &Path) -> Result<(u32, u32), GitError> {
    let output = run_git(
        repository,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
        64 * 1024,
    )
    .await?;
    let text = String::from_utf8_lossy(&output.bytes);
    let mut counts = text
        .split_whitespace()
        .filter_map(|value| value.parse::<u32>().ok());
    Ok((counts.next().unwrap_or(0), counts.next().unwrap_or(0)))
}
pub(super) fn parse_status(output: &LimitedOutput) -> Vec<GitFileStatus> {
    let records = output.bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        if record.len() < 4 {
            continue;
        }
        let index_status = (record[0] as char).to_string();
        let worktree_status = (record[1] as char).to_string();
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        let renamed = matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C');
        let original_path = if renamed && index < records.len() {
            let original = String::from_utf8_lossy(records[index]).into_owned();
            index += 1;
            Some(original)
        } else {
            None
        };
        files.push(GitFileStatus {
            path,
            index_status,
            worktree_status,
            original_path,
        });
    }
    files
}
