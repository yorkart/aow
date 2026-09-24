use std::{
    collections::{BTreeSet, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use aow_protocol::{
    GitCommit, GitCommitDetail, GitCommitFile, GitCommitFiles, GitCommitStats, GitDiff,
    GitFileStatus, GitIdentity, GitIgnoredPaths, GitLog, GitStatus, RepositorySummary,
};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};
use walkdir::{DirEntry, WalkDir};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const SMALL_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
const DIFF_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("path must be absolute: {0}")]
    PathNotAbsolute(String),
    #[error("path is not inside a Git repository: {0}")]
    NotRepository(String),
    #[error("git command timed out")]
    Timeout,
    #[error("git command failed: {0}")]
    Command(String),
    #[error("git output was not valid UTF-8")]
    InvalidUtf8,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),
}

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

async fn repository_root_for_path(path: &Path) -> Result<PathBuf, GitError> {
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

pub async fn log(repository: impl AsRef<Path>, limit: usize) -> Result<GitLog, GitError> {
    let repository = absolute(repository)?;
    match run_git(&repository, &["rev-parse", "--verify", "HEAD"], 64 * 1024).await {
        Ok(_) => {}
        Err(GitError::Command(_)) => {
            return Ok(GitLog {
                repository: repository.to_string_lossy().into_owned(),
                upstream: None,
                upstream_commit: None,
                commits: Vec::new(),
            });
        }
        Err(error) => return Err(error),
    }
    let limit = limit.clamp(1, 500);
    let limit_arg = limit.to_string();
    let output = run_git(
        &repository,
        &[
            "log",
            "--topo-order",
            "--date=iso-strict",
            "--format=%H%x1f%h%x1f%an%x1f%aI%x1f%s%x1f%P%x1e",
            "-n",
            &limit_arg,
        ],
        SMALL_OUTPUT_LIMIT,
    )
    .await?;
    let text = String::from_utf8(output.bytes).map_err(|_| GitError::InvalidUtf8)?;
    let mut commits = text
        .split('')
        .filter_map(|record| {
            let record = record.trim_matches(['\n', '\r']);
            if record.is_empty() {
                return None;
            }
            let fields = record.splitn(6, '').collect::<Vec<_>>();
            (fields.len() == 6).then(|| GitCommit {
                id: fields[0].to_owned(),
                short_id: fields[1].to_owned(),
                author: fields[2].to_owned(),
                authored_at: fields[3].to_owned(),
                subject: fields[4].to_owned(),
                parents: fields[5]
                    .split_whitespace()
                    .map(ToOwned::to_owned)
                    .collect(),
                is_pushed: false,
            })
        })
        .collect::<Vec<_>>();

    let (upstream, upstream_commit) = match upstream_snapshot(&repository).await? {
        Some(snapshot) => snapshot,
        None => {
            return Ok(GitLog {
                repository: repository.to_string_lossy().into_owned(),
                upstream: None,
                upstream_commit: None,
                commits,
            });
        }
    };
    if !commits.is_empty() {
        let mut owned_args = vec!["rev-list".to_owned(), "--no-walk".to_owned()];
        owned_args.extend(commits.iter().map(|commit| commit.id.clone()));
        owned_args.push("--not".to_owned());
        owned_args.push(upstream_commit.clone());
        let args = owned_args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = run_git(&repository, &args, SMALL_OUTPUT_LIMIT).await?;
        let outgoing = String::from_utf8(output.bytes)
            .map_err(|_| GitError::InvalidUtf8)?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect::<HashSet<_>>();
        for commit in &mut commits {
            commit.is_pushed = !outgoing.contains(&commit.id);
        }
    }
    Ok(GitLog {
        repository: repository.to_string_lossy().into_owned(),
        upstream: Some(upstream),
        upstream_commit: Some(upstream_commit),
        commits,
    })
}

async fn upstream_snapshot(repository: &Path) -> Result<Option<(String, String)>, GitError> {
    let upstream = match run_git(
        repository,
        &[
            "rev-parse",
            "--verify",
            "--abbrev-ref=strict",
            "@{upstream}",
        ],
        64 * 1024,
    )
    .await
    {
        Ok(output) => String::from_utf8(output.bytes)
            .map_err(|_| GitError::InvalidUtf8)?
            .trim()
            .to_owned(),
        Err(GitError::Command(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if upstream.is_empty() {
        return Ok(None);
    }

    let output = match run_git(
        repository,
        &["rev-parse", "--verify", "@{upstream}^{commit}"],
        64 * 1024,
    )
    .await
    {
        Ok(output) => output,
        Err(GitError::Command(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let commit = String::from_utf8(output.bytes)
        .map_err(|_| GitError::InvalidUtf8)?
        .trim()
        .to_owned();
    if commit.is_empty() {
        Ok(None)
    } else {
        Ok(Some((upstream, commit)))
    }
}

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

pub async fn commit_detail(
    repository: impl AsRef<Path>,
    commit: &str,
) -> Result<GitCommitDetail, GitError> {
    let repository = absolute(repository)?;
    validate_object_id(commit)?;
    let id = canonical_commit(&repository, commit).await?;
    let metadata = run_git(
        &repository,
        &[
            "show",
            "-s",
            "--no-show-signature",
            "--format=format:%H%x00%h%x00%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI%x00%s%x00%b%x00%P",
            &id,
        ],
        SMALL_OUTPUT_LIMIT,
    )
    .await?;
    if metadata.truncated {
        return Err(GitError::Command(
            "git commit metadata output exceeded limit".to_owned(),
        ));
    }
    let metadata = String::from_utf8(metadata.bytes).map_err(|_| GitError::InvalidUtf8)?;
    let fields = metadata.splitn(11, '\0').collect::<Vec<_>>();
    if fields.len() != 11 || fields[0] != id {
        return Err(GitError::Command(
            "invalid git commit metadata output".to_owned(),
        ));
    }

    let parent = first_parent(&repository, &id).await;
    let mut stats_args = vec!["diff-tree".to_owned()];
    if parent.is_none() {
        stats_args.push("--root".to_owned());
    }
    stats_args.extend(
        ["--no-commit-id", "--numstat", "-z", "-r", "-M"]
            .into_iter()
            .map(ToOwned::to_owned),
    );
    if let Some(parent) = parent {
        stats_args.push(parent);
    }
    stats_args.push(id.clone());
    let stats_refs = stats_args.iter().map(String::as_str).collect::<Vec<_>>();
    let stats_output = run_git(&repository, &stats_refs, SMALL_OUTPUT_LIMIT).await?;
    if stats_output.truncated {
        return Err(GitError::Command(
            "git commit stats output exceeded limit".to_owned(),
        ));
    }
    let stats = parse_numstat(&stats_output.bytes)?;
    let refs = exact_refs(&repository, &id).await?;
    let upstream = upstream_snapshot(&repository)
        .await?
        .map(|(upstream, _)| upstream);
    let remote_name = preferred_remote(&repository).await;

    Ok(GitCommitDetail {
        repository: repository.to_string_lossy().into_owned(),
        id,
        short_id: fields[1].to_owned(),
        author: GitIdentity {
            name: fields[2].to_owned(),
            email: fields[3].to_owned(),
            date: fields[4].to_owned(),
        },
        committer: GitIdentity {
            name: fields[5].to_owned(),
            email: fields[6].to_owned(),
            date: fields[7].to_owned(),
        },
        subject: fields[8].to_owned(),
        body: fields[9].trim_end_matches(['\r', '\n']).to_owned(),
        parents: fields[10]
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect(),
        refs,
        stats,
        upstream,
        remote_name,
        // Web URLs belong to the configured Provider, not to local Git.
        remote_url: None,
        commit_url: None,
    })
}

async fn canonical_commit(repository: &Path, commit: &str) -> Result<String, GitError> {
    let expression = format!("{commit}^{{commit}}");
    let output = run_git(
        repository,
        &["rev-parse", "--verify", &expression],
        64 * 1024,
    )
    .await?;
    if output.truncated {
        return Err(GitError::Command(
            "git commit id output exceeded limit".to_owned(),
        ));
    }
    let id = String::from_utf8(output.bytes)
        .map_err(|_| GitError::InvalidUtf8)?
        .trim()
        .to_owned();
    validate_object_id(&id)?;
    Ok(id)
}

async fn exact_refs(repository: &Path, commit: &str) -> Result<Vec<String>, GitError> {
    let output = run_git(
        repository,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(objectname)%00%(*objectname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
        SMALL_OUTPUT_LIMIT,
    )
    .await?;
    if output.truncated {
        return Err(GitError::Command(
            "git commit refs output exceeded limit".to_owned(),
        ));
    }
    let text = String::from_utf8(output.bytes).map_err(|_| GitError::InvalidUtf8)?;
    let mut refs = BTreeSet::new();
    for line in text.lines() {
        let fields = line.splitn(3, '\0').collect::<Vec<_>>();
        if fields.len() == 3
            && (fields[1] == commit || fields[2] == commit)
            && !fields[0].is_empty()
        {
            refs.insert(fields[0].to_owned());
        }
    }
    if optional_git_line(repository, &["rev-parse", "--verify", "HEAD^{commit}"])
        .await
        .as_deref()
        == Some(commit)
    {
        refs.insert("HEAD".to_owned());
    }
    Ok(refs.into_iter().collect())
}

fn parse_numstat(bytes: &[u8]) -> Result<GitCommitStats, GitError> {
    let mut stats = GitCommitStats {
        files_changed: 0,
        insertions: 0,
        deletions: 0,
        binary_files: 0,
    };
    let records = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        if record.is_empty() {
            continue;
        }
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let insertions = fields.next();
        let deletions = fields.next();
        let path = fields.next();
        let (Some(insertions), Some(deletions), Some(path)) = (insertions, deletions, path) else {
            return Err(GitError::Command("invalid git numstat output".to_owned()));
        };
        // With `-z`, a rename/copy has an empty path in the numstat record,
        // followed by the old and new paths as two additional NUL records.
        if path.is_empty() {
            if index + 1 >= records.len()
                || records[index].is_empty()
                || records[index + 1].is_empty()
            {
                return Err(GitError::Command(
                    "invalid git numstat rename output".to_owned(),
                ));
            }
            index += 2;
        }
        stats.files_changed = stats.files_changed.saturating_add(1);
        if insertions == b"-" || deletions == b"-" {
            stats.binary_files = stats.binary_files.saturating_add(1);
            continue;
        }
        let parse = |value: &[u8]| {
            std::str::from_utf8(value)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| GitError::Command("invalid git numstat output".to_owned()))
        };
        stats.insertions = stats.insertions.saturating_add(parse(insertions)?);
        stats.deletions = stats.deletions.saturating_add(parse(deletions)?);
    }
    Ok(stats)
}

async fn preferred_remote(repository: &Path) -> Option<String> {
    let tracking = tracking_remote(repository).await;
    let mut candidates = tracking.into_iter().collect::<Vec<_>>();
    if !candidates.iter().any(|name| name == "origin") {
        candidates.push("origin".to_owned());
    }
    if let Some(remotes) = optional_git_lines(repository, &["remote"]).await {
        for name in remotes {
            if !candidates.contains(&name) {
                candidates.push(name);
            }
        }
    }
    for name in candidates {
        if optional_git_line(repository, &["remote", "get-url", &name])
            .await
            .is_some()
        {
            return Some(name);
        }
    }
    None
}

async fn tracking_remote(repository: &Path) -> Option<String> {
    let branch =
        optional_git_line(repository, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await?;
    let key = format!("branch.{branch}.remote");
    optional_git_line(repository, &["config", "--get", &key])
        .await
        .filter(|remote| remote != ".")
}

async fn optional_git_line(repository: &Path, args: &[&str]) -> Option<String> {
    let output = run_git(repository, args, 64 * 1024).await.ok()?;
    let value = String::from_utf8(output.bytes).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

async fn optional_git_lines(repository: &Path, args: &[&str]) -> Option<Vec<String>> {
    let output = run_git(repository, args, 64 * 1024).await.ok()?;
    if output.truncated {
        return None;
    }
    Some(
        String::from_utf8(output.bytes)
            .ok()?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    )
}

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

fn validate_object_id(value: &str) -> Result<(), GitError> {
    if (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitError::Command("invalid commit id".to_owned()))
    }
}

async fn first_parent(repository: &Path, commit: &str) -> Option<String> {
    let output = run_git(
        repository,
        &["rev-parse", &format!("{commit}^1")],
        64 * 1024,
    )
    .await
    .ok()?;
    Some(String::from_utf8_lossy(&output.bytes).trim().to_owned())
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

async fn git_text(repository: &Path, args: &[&str]) -> Option<String> {
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

async fn current_branch(repository: &Path) -> Result<String, GitError> {
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

fn parse_status(output: &LimitedOutput) -> Vec<GitFileStatus> {
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

struct LimitedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn run_git(
    repository: &Path,
    args: &[&str],
    limit: usize,
) -> Result<LimitedOutput, GitError> {
    run_git_with_environment(repository, args, limit, &[]).await
}

async fn run_git_with_environment(
    repository: &Path,
    args: &[&str],
    limit: usize,
    environment: &[(&str, &str)],
) -> Result<LimitedOutput, GitError> {
    run_git_with_timeout(repository, args, limit, environment, COMMAND_TIMEOUT).await
}

async fn run_git_with_timeout(
    repository: &Path,
    args: &[&str],
    limit: usize,
    environment: &[(&str, &str)],
    command_timeout: Duration,
) -> Result<LimitedOutput, GitError> {
    let future = async {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .envs(environment.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdout = child.stdout.take().expect("stdout configured");
        let stderr = child.stderr.take().expect("stderr configured");
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(256 * 1024).read_to_end(&mut bytes).await;
            bytes
        });
        let mut bytes = Vec::with_capacity(limit.min(256 * 1024));
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut truncated = false;
        loop {
            let count = stdout.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            let remaining = limit.saturating_sub(bytes.len());
            let take = count.min(remaining);
            bytes.extend_from_slice(&buffer[..take]);
            if take < count || bytes.len() >= limit {
                truncated = true;
                child.kill().await?;
                break;
            }
        }
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if !status.success() && !truncated {
            let message = String::from_utf8_lossy(&stderr).trim().to_owned();
            return Err(GitError::Command(if message.is_empty() {
                format!("git exited with {status}")
            } else {
                message
            }));
        }
        Ok(LimitedOutput { bytes, truncated })
    };

    timeout(command_timeout, future)
        .await
        .map_err(|_| GitError::Timeout)?
}

async fn run_git_check_ignore(
    repository: &Path,
    input: Vec<u8>,
) -> Result<LimitedOutput, GitError> {
    let future = async {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["check-ignore", "-z", "--stdin"])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdin = child.stdin.take().expect("stdin configured");
        let stdin_task = tokio::spawn(async move { stdin.write_all(&input).await });
        let mut stdout = child.stdout.take().expect("stdout configured");
        let stderr = child.stderr.take().expect("stderr configured");
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(256 * 1024).read_to_end(&mut bytes).await;
            bytes
        });
        let mut bytes = Vec::with_capacity(64 * 1024);
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut truncated = false;
        loop {
            let count = stdout.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            let remaining = SMALL_OUTPUT_LIMIT.saturating_sub(bytes.len());
            let take = count.min(remaining);
            bytes.extend_from_slice(&buffer[..take]);
            if take < count || bytes.len() >= SMALL_OUTPUT_LIMIT {
                truncated = true;
                child.kill().await?;
                break;
            }
        }
        stdin_task
            .await
            .map_err(|error| GitError::Command(error.to_string()))??;
        let status = child.wait().await?;
        let stderr = stderr_task.await.unwrap_or_default();
        if !matches!(status.code(), Some(0 | 1)) && !truncated {
            let message = String::from_utf8_lossy(&stderr).trim().to_owned();
            return Err(GitError::Command(if message.is_empty() {
                format!("git exited with {status}")
            } else {
                message
            }));
        }
        Ok(LimitedOutput { bytes, truncated })
    };

    timeout(COMMAND_TIMEOUT, future)
        .await
        .map_err(|_| GitError::Timeout)?
}

fn absolute(path: impl AsRef<Path>) -> Result<PathBuf, GitError> {
    let path = path.as_ref();
    if !path.is_absolute() {
        return Err(GitError::PathNotAbsolute(
            path.to_string_lossy().into_owned(),
        ));
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    #[test]
    fn parses_porcelain_status() {
        let output = LimitedOutput {
            bytes: b" M file.txt\0?? new.txt\0R  renamed.txt\0old.txt\0".to_vec(),
            truncated: false,
        };
        let files = parse_status(&output);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "file.txt");
        assert_eq!(files[1].worktree_status, "?");
        assert_eq!(files[2].original_path.as_deref(), Some("old.txt"));
    }

    #[test]
    fn parses_numstat_for_text_binary_and_renamed_files() {
        let stats =
            parse_numstat(b"3\t1\ttext.txt\0-\t-\tbinary.bin\x002\t0\t\0old.txt\0new.txt\0")
                .unwrap();
        assert_eq!(stats.files_changed, 3);
        assert_eq!(stats.insertions, 5);
        assert_eq!(stats.deletions, 1);
        assert_eq!(stats.binary_files, 1);
    }

    fn git(repo: &Path, args: &[&str]) {
        let status = StdCommand::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn git_output(repo: &Path, args: &[&str]) -> String {
        let output = StdCommand::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[tokio::test]
    async fn checks_ignored_paths_in_one_repository_query() {
        let parent = TempDir::new().unwrap();
        let repo = parent.path().join("ignored paths");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join(".gitignore"), "ignored-dir/\n*.tmp\n").unwrap();
        std::fs::create_dir(repo.join("ignored-dir")).unwrap();
        std::fs::write(repo.join("ignored.tmp"), "ignored\n").unwrap();
        std::fs::write(repo.join("tracked.tmp"), "tracked\n").unwrap();
        git(&repo, &["add", "-f", "tracked.tmp"]);

        let result = ignored_paths(
            &repo,
            [
                repo.join("ignored-dir"),
                repo.join("ignored.tmp"),
                repo.join("tracked.tmp"),
                repo.join(".git"),
                parent.path().join("outside.tmp"),
            ],
        )
        .await
        .unwrap();

        assert_eq!(
            result.repository.as_deref(),
            Some(repo.to_string_lossy().as_ref())
        );
        assert_eq!(
            result.ignored,
            vec![
                repo.join("ignored-dir").to_string_lossy().into_owned(),
                repo.join("ignored.tmp").to_string_lossy().into_owned(),
            ]
        );

        let non_repository = ignored_paths(parent.path(), [parent.path().join("outside.tmp")])
            .await
            .unwrap();
        assert_eq!(
            non_repository,
            GitIgnoredPaths {
                repository: None,
                ignored: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn falls_back_to_a_non_origin_remote() {
        let parent = TempDir::new().unwrap();
        let repo = parent.path().join("remote-fallback");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Remote Test"]);
        git(&repo, &["config", "user.email", "remote@example.com"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "remote"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "mirror",
                "git@git.example.com:acme/project.git",
            ],
        );
        let id = git_output(&repo, &["rev-parse", "HEAD"]);
        let detail = commit_detail(&repo, &id).await.unwrap();
        assert_eq!(detail.remote_name.as_deref(), Some("mirror"));
        assert!(detail.remote_url.is_none());
        assert!(detail.commit_url.is_none());
    }

    #[tokio::test]
    async fn exercises_real_repository_status_log_and_diff() {
        let parent = TempDir::new().unwrap();
        let repo = parent.path().join("sample-repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "AOW Test"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        std::fs::write(repo.join("tracked.txt"), "first\n").unwrap();
        git(&repo, &["add", "tracked.txt"]);
        git(&repo, &["commit", "-q", "-m", "initial commit"]);

        std::fs::write(repo.join("tracked.txt"), "first\nsecond\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "new\n").unwrap();

        let repositories = discover_repositories(parent.path(), 3).await.unwrap();
        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].path, repo.to_string_lossy());
        let repository_status = status(&repo).await.unwrap();
        assert!(
            repository_status
                .files
                .iter()
                .any(|file| file.path == "tracked.txt")
        );
        assert!(
            repository_status
                .files
                .iter()
                .any(|file| file.path == "untracked.txt")
        );

        let history = log(&repo, 10).await.unwrap();
        assert_eq!(history.commits.len(), 1);
        assert_eq!(history.commits[0].subject, "initial commit");
        assert_eq!(history.upstream, None);
        assert_eq!(history.upstream_commit, None);
        assert!(!history.commits[0].is_pushed);
        let root_commit = history.commits[0].id.clone();
        git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
        git(
            &repo,
            &["update-ref", "refs/remotes/origin/main", &root_commit],
        );
        git(&repo, &["config", "branch.main.remote", "origin"]);
        git(&repo, &["config", "branch.main.merge", "refs/heads/main"]);
        let tracked_history = log(&repo, 10).await.unwrap();
        assert_eq!(tracked_history.upstream.as_deref(), Some("origin/main"));
        assert_eq!(
            tracked_history.upstream_commit.as_deref(),
            Some(root_commit.as_str())
        );
        assert!(tracked_history.commits[0].is_pushed);
        let root_files = commit_files(&repo, &root_commit).await.unwrap();
        assert_eq!(root_files.files.len(), 1);
        assert_eq!(root_files.files[0].path, "tracked.txt");
        assert_eq!(root_files.files[0].status, "A");
        let root_diff = commit_diff(&repo, &root_commit, "tracked.txt", None)
            .await
            .unwrap();
        assert_eq!(root_diff.original.as_deref(), Some(""));
        assert_eq!(root_diff.modified.as_deref(), Some("first\n"));

        let patch = diff(&repo, Some("tracked.txt"), false).await.unwrap();
        assert!(patch.patch.contains("+second"));
        assert_eq!(patch.original.as_deref(), Some("first\n"));
        assert_eq!(patch.modified.as_deref(), Some("first\nsecond\n"));

        git(&repo, &["add", "tracked.txt"]);
        let staged = diff(&repo, Some("tracked.txt"), true).await.unwrap();
        assert!(staged.patch.contains("+second"));
        assert_eq!(staged.original.as_deref(), Some("first\n"));
        assert_eq!(staged.modified.as_deref(), Some("first\nsecond\n"));

        git(&repo, &["commit", "-q", "-m", "second commit"]);
        let history = log(&repo, 10).await.unwrap();
        let second_commit = &history.commits[0];
        assert!(!second_commit.is_pushed);
        assert!(
            history
                .commits
                .iter()
                .find(|commit| commit.id == root_commit)
                .unwrap()
                .is_pushed
        );
        let branch_status = status(&repo).await.unwrap();
        assert_eq!((branch_status.ahead, branch_status.behind), (1, 0));
        let files = commit_files(&repo, &second_commit.id).await.unwrap();
        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, "tracked.txt");
        assert_eq!(files.files[0].status, "M");
        let commit_patch = commit_diff(&repo, &second_commit.id, "tracked.txt", None)
            .await
            .unwrap();
        assert!(commit_patch.patch.contains("+second"));
        assert_eq!(commit_patch.original.as_deref(), Some("first\n"));
        assert_eq!(commit_patch.modified.as_deref(), Some("first\nsecond\n"));

        git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let pushed_history = log(&repo, 10).await.unwrap();
        assert!(pushed_history.commits.iter().all(|commit| commit.is_pushed));
        assert_eq!(
            pushed_history.upstream_commit.as_deref(),
            Some(pushed_history.commits[0].id.as_str())
        );
    }

    #[tokio::test]
    async fn normalizes_worktree_content_before_rendering_a_diff() {
        let parent = TempDir::new().unwrap();
        let repo = parent.path().join("eol-repository");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "AOW Test"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "core.autocrlf", "true"]);
        std::fs::write(repo.join("tracked.txt"), "first\nsecond\nthird\n").unwrap();
        git(&repo, &["add", "tracked.txt"]);
        git(&repo, &["commit", "-q", "-m", "initial commit"]);

        std::fs::write(repo.join("tracked.txt"), "first\r\nSECOND\r\nthird\r\n").unwrap();

        let rendered = diff(&repo, Some("tracked.txt"), false).await.unwrap();
        assert!(rendered.patch.contains("-second\n+SECOND"));
        assert_eq!(rendered.original.as_deref(), Some("first\nsecond\nthird\n"));
        assert_eq!(rendered.modified.as_deref(), Some("first\nSECOND\nthird\n"));
    }

    #[tokio::test]
    async fn lists_merge_commit_files_and_keeps_empty_commits_empty() {
        let parent = TempDir::new().unwrap();
        let repo = parent.path().join("merge-repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "AOW Test"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        git(&repo, &["add", "base.txt"]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);

        git(&repo, &["switch", "-q", "-c", "feature"]);
        std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
        git(&repo, &["add", "feature.txt"]);
        git(&repo, &["commit", "-q", "-m", "feature"]);

        git(&repo, &["switch", "-q", "main"]);
        std::fs::write(repo.join("main.txt"), "main\n").unwrap();
        git(&repo, &["add", "main.txt"]);
        git(&repo, &["commit", "-q", "-m", "main"]);
        let feature_commit = git_output(&repo, &["rev-parse", "feature"]);
        git(
            &repo,
            &["update-ref", "refs/remotes/origin/main", &feature_commit],
        );
        git(&repo, &["config", "branch.main.remote", "origin"]);
        git(&repo, &["config", "branch.main.merge", "refs/heads/main"]);
        let diverged = log(&repo, 10).await.unwrap();
        assert_eq!(diverged.upstream.as_deref(), Some("origin/main"));
        assert_eq!(
            diverged.upstream_commit.as_deref(),
            Some(feature_commit.as_str())
        );
        assert!(
            !diverged
                .commits
                .iter()
                .find(|commit| commit.subject == "main")
                .unwrap()
                .is_pushed
        );
        assert!(
            diverged
                .commits
                .iter()
                .find(|commit| commit.subject == "base")
                .unwrap()
                .is_pushed
        );
        git(&repo, &["merge", "-q", "--no-ff", "feature", "-m", "merge"]);

        let history = log(&repo, 1).await.unwrap();
        assert!(!history.commits[0].is_pushed);
        let files = commit_files(&repo, &history.commits[0].id).await.unwrap();
        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].path, "feature.txt");
        assert_eq!(files.files[0].status, "A");

        let merged_history = log(&repo, 10).await.unwrap();
        assert_eq!(
            merged_history
                .commits
                .iter()
                .find(|commit| commit.subject == "merge")
                .unwrap()
                .parents
                .len(),
            2
        );
        assert!(
            merged_history
                .commits
                .iter()
                .find(|commit| commit.subject == "feature")
                .unwrap()
                .is_pushed
        );
        assert!(
            !merged_history
                .commits
                .iter()
                .find(|commit| commit.subject == "main")
                .unwrap()
                .is_pushed
        );
        assert!(
            !merged_history
                .commits
                .iter()
                .find(|commit| commit.subject == "merge")
                .unwrap()
                .is_pushed
        );
        git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let pushed_merge = log(&repo, 10).await.unwrap();
        assert!(pushed_merge.commits.iter().all(|commit| commit.is_pushed));

        git(&repo, &["commit", "-q", "--allow-empty", "-m", "empty"]);
        let history = log(&repo, 1).await.unwrap();
        let files = commit_files(&repo, &history.commits[0].id).await.unwrap();
        assert!(files.files.is_empty());
    }

    #[tokio::test]
    async fn returns_real_commit_detail_metadata_stats_parents_refs_and_remote() {
        let parent = TempDir::new().unwrap();
        let repo = parent.path().join("detail-repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Detail Committer"]);
        git(&repo, &["config", "user.email", "committer@example.com"]);
        std::fs::write(repo.join("tracked.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.join("binary.bin"), b"before\0binary").unwrap();
        git(&repo, &["add", "tracked.txt", "binary.bin"]);
        git(
            &repo,
            &[
                "commit",
                "-q",
                "--author",
                "Root Author <root@example.com>",
                "-m",
                "root subject",
                "-m",
                "root body line one\nroot body line two",
            ],
        );
        let root_id = git_output(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["tag", "root-tag", &root_id]);
        git(
            &repo,
            &["tag", "-a", "annotated-root", "-m", "annotated", &root_id],
        );
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://fallback:secret@git.example.com/acme/fallback.git",
            ],
        );
        git(
            &repo,
            &[
                "remote",
                "add",
                "review",
                "git@git.example.com:acme/detail.git",
            ],
        );
        git(&repo, &["config", "branch.main.remote", "review"]);
        git(&repo, &["config", "branch.main.merge", "refs/heads/main"]);

        let root = commit_detail(&repo, &root_id[..12]).await.unwrap();
        assert_eq!(root.id, root_id);
        assert_eq!(root.subject, "root subject");
        assert!(
            root.body
                .starts_with("root body line one\nroot body line two")
        );
        assert_eq!(root.author.name, "Root Author");
        assert_eq!(root.author.email, "root@example.com");
        assert_eq!(root.committer.name, "Detail Committer");
        assert_eq!(root.committer.email, "committer@example.com");
        assert!(root.author.date.contains('T'));
        assert!(root.parents.is_empty());
        assert_eq!(root.stats.files_changed, 2);
        assert_eq!((root.stats.insertions, root.stats.deletions), (2, 0));
        assert_eq!(root.stats.binary_files, 1);
        assert_eq!(
            root.refs,
            vec!["HEAD", "annotated-root", "main", "root-tag"]
        );
        assert_eq!(root.upstream, None);
        assert_eq!(root.remote_name.as_deref(), Some("review"));
        assert!(root.remote_url.is_none());
        assert!(root.commit_url.is_none());

        git(&repo, &["switch", "-q", "-c", "feature"]);
        std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
        git(&repo, &["add", "feature.txt"]);
        git(&repo, &["commit", "-q", "-m", "feature"]);
        git(&repo, &["switch", "-q", "main"]);
        std::fs::write(repo.join("tracked.txt"), "one\nchanged\nthree\n").unwrap();
        git(&repo, &["add", "tracked.txt"]);
        git(&repo, &["commit", "-q", "-m", "main change"]);
        git(
            &repo,
            &["merge", "-q", "--no-ff", "feature", "-m", "merge detail"],
        );
        let merge_id = git_output(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["branch", "exact-merge", &merge_id]);
        git(&repo, &["branch", "not-exact", &root_id]);

        let merge = commit_detail(&repo, &merge_id).await.unwrap();
        assert_eq!(merge.parents.len(), 2);
        assert_eq!(merge.stats.files_changed, 1);
        assert_eq!((merge.stats.insertions, merge.stats.deletions), (1, 0));
        assert_eq!(merge.stats.binary_files, 0);
        assert!(merge.refs.contains(&"HEAD".to_owned()));
        assert!(merge.refs.contains(&"exact-merge".to_owned()));
        assert!(merge.refs.contains(&"main".to_owned()));
        assert!(!merge.refs.contains(&"not-exact".to_owned()));
    }
}
