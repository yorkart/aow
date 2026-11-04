use std::path::Path;

use aow_protocol::GitDiff;

use super::{
    GitError,
    command::{
        DIFF_OUTPUT_LIMIT, LimitedOutput, SMALL_OUTPUT_LIMIT, absolute, run_git,
        run_git_with_environment,
    },
};

pub async fn diff(
    repository: impl AsRef<Path>,
    path: Option<&str>,
    staged: bool,
) -> Result<GitDiff, GitError> {
    let repository = absolute(repository)?;
    let mut owned_args = vec![
        "diff".to_owned(),
        "--no-ext-diff".to_owned(),
        "--no-color".to_owned(),
    ];
    if staged {
        owned_args.push("--cached".to_owned());
    }
    if let Some(path) = path {
        owned_args.push("--".to_owned());
        owned_args.push(path.to_owned());
    }
    let args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_git(&repository, &args, DIFF_OUTPUT_LIMIT).await?;
    let patch = String::from_utf8_lossy(&output.bytes).into_owned();
    let (original, modified) = match path {
        Some(path) => diff_file_versions(&repository, path, staged).await,
        None => (None, None),
    };
    Ok(GitDiff {
        repository: repository.to_string_lossy().into_owned(),
        path: path.map(ToOwned::to_owned),
        staged,
        patch,
        truncated: output.truncated,
        original,
        modified,
    })
}
async fn diff_file_versions(
    repository: &Path,
    path: &str,
    staged: bool,
) -> (Option<String>, Option<String>) {
    let original_args = if staged {
        vec!["show".to_owned(), format!("HEAD:{path}")]
    } else {
        vec!["show".to_owned(), format!(":{path}")]
    };
    let original_refs = original_args.iter().map(String::as_str).collect::<Vec<_>>();
    let original = git_text(repository, &original_refs)
        .await
        .or(Some(String::new()));
    let modified = if staged {
        let args = ["show".to_owned(), format!(":{path}")];
        let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
        git_text(repository, &refs).await.or(Some(String::new()))
    } else {
        normalized_worktree_text(repository, path).await
    };
    (original, modified)
}

pub(super) async fn git_text(repository: &Path, args: &[&str]) -> Option<String> {
    let output = run_git(repository, args, DIFF_OUTPUT_LIMIT).await.ok()?;
    text_from_git_output(output)
}

async fn git_text_with_environment(
    repository: &Path,
    args: &[&str],
    environment: &[(&str, &str)],
) -> Option<String> {
    let output = run_git_with_environment(repository, args, DIFF_OUTPUT_LIMIT, environment)
        .await
        .ok()?;
    text_from_git_output(output)
}

fn text_from_git_output(output: LimitedOutput) -> Option<String> {
    if output.bytes.contains(&0) {
        return None;
    }
    String::from_utf8(output.bytes).ok()
}

async fn normalized_worktree_text(repository: &Path, path: &str) -> Option<String> {
    let worktree_path = repository.join(path);
    let metadata = tokio::fs::metadata(&worktree_path).await.ok();
    if metadata
        .as_ref()
        .is_some_and(|item| item.len() > DIFF_OUTPUT_LIMIT as u64)
    {
        return None;
    }
    if metadata.is_none() {
        return Some(String::new());
    }

    // The raw worktree bytes can differ from the content Git compares because of
    // eol attributes and clean filters. Stage the file in isolated index and
    // object directories so `git show` yields the same normalized representation
    // as `git diff`, without touching the repository's real Git state.
    let temporary = tempfile::tempdir().ok()?;
    let index_path = temporary.path().join("index");
    let objects_path = temporary.path().join("objects");
    std::fs::create_dir(&objects_path).ok()?;
    let index_path = index_path.to_string_lossy();
    let objects_path = objects_path.to_string_lossy();
    let environment = [
        ("GIT_INDEX_FILE", index_path.as_ref()),
        ("GIT_OBJECT_DIRECTORY", objects_path.as_ref()),
    ];
    run_git_with_environment(
        repository,
        &["add", "--", path],
        SMALL_OUTPUT_LIMIT,
        &environment,
    )
    .await
    .ok()?;
    git_text_with_environment(repository, &["show", &format!(":{path}")], &environment).await
}
