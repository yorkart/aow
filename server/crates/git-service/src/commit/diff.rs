use std::path::Path;

use aow_protocol::GitDiff;

use crate::{
    GitError,
    command::{DIFF_OUTPUT_LIMIT, absolute, run_git},
    diff::git_text,
};

use super::common::{first_parent, validate_object_id};

pub async fn commit_diff(
    repository: impl AsRef<Path>,
    commit: &str,
    path: &str,
    original_path: Option<&str>,
) -> Result<GitDiff, GitError> {
    let repository = absolute(repository)?;
    validate_object_id(commit)?;
    let parent = first_parent(&repository, commit).await;
    let parent_path = original_path.unwrap_or(path);
    let patch = match parent.as_deref() {
        Some(parent) => {
            run_git(
                &repository,
                &[
                    "diff",
                    "--no-ext-diff",
                    "--no-color",
                    parent,
                    commit,
                    "--",
                    path,
                ],
                DIFF_OUTPUT_LIMIT,
            )
            .await?
        }
        None => {
            run_git(
                &repository,
                &[
                    "show",
                    "--format=",
                    "--no-ext-diff",
                    "--no-color",
                    commit,
                    "--",
                    path,
                ],
                DIFF_OUTPUT_LIMIT,
            )
            .await?
        }
    };
    let original = match parent.as_deref() {
        Some(parent) => git_text(&repository, &["show", &format!("{parent}:{parent_path}")]).await,
        None => Some(String::new()),
    };
    let modified = git_text(&repository, &["show", &format!("{commit}:{path}")])
        .await
        .or(Some(String::new()));
    Ok(GitDiff {
        repository: repository.to_string_lossy().into_owned(),
        path: Some(path.to_owned()),
        staged: false,
        patch: String::from_utf8_lossy(&patch.bytes).into_owned(),
        truncated: patch.truncated,
        original,
        modified,
    })
}
