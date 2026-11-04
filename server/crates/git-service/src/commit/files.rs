use std::path::Path;

use aow_protocol::{GitCommitFile, GitCommitFiles};

use crate::{
    GitError,
    command::{SMALL_OUTPUT_LIMIT, absolute, run_git},
};

use super::common::{first_parent, validate_object_id};

pub async fn commit_files(
    repository: impl AsRef<Path>,
    commit: &str,
) -> Result<GitCommitFiles, GitError> {
    let repository = absolute(repository)?;
    validate_object_id(commit)?;
    let parent = first_parent(&repository, commit).await;
    let mut owned_args = vec!["diff-tree".to_owned()];
    if parent.is_none() {
        owned_args.push("--root".to_owned());
    }
    owned_args.extend(
        ["--no-commit-id", "--name-status", "-z", "-r"]
            .into_iter()
            .map(ToOwned::to_owned),
    );
    if let Some(parent) = parent {
        owned_args.push(parent);
    }
    owned_args.push(commit.to_owned());
    let args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_git(&repository, &args, SMALL_OUTPUT_LIMIT).await?;
    Ok(GitCommitFiles {
        repository: repository.to_string_lossy().into_owned(),
        commit: commit.to_owned(),
        files: parse_name_status(&output.bytes),
    })
}

fn parse_name_status(bytes: &[u8]) -> Vec<GitCommitFile> {
    let records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut files = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let status = String::from_utf8_lossy(records[index]).into_owned();
        index += 1;
        if index >= records.len() {
            break;
        }
        let first_path = String::from_utf8_lossy(records[index]).into_owned();
        index += 1;
        let renamed = status.starts_with('R') || status.starts_with('C');
        let (path, original_path) = if renamed && index < records.len() {
            let path = String::from_utf8_lossy(records[index]).into_owned();
            index += 1;
            (path, Some(first_path))
        } else {
            (first_path, None)
        };
        files.push(GitCommitFile {
            path,
            status,
            original_path,
        });
    }
    files
}
