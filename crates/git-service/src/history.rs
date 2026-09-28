use std::{collections::HashSet, path::Path};

use aow_protocol::{GitCommit, GitLog};

use super::{
    GitError,
    command::{SMALL_OUTPUT_LIMIT, absolute, run_git},
};

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

pub(super) async fn upstream_snapshot(
    repository: &Path,
) -> Result<Option<(String, String)>, GitError> {
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
pub(super) async fn preferred_remote(repository: &Path) -> Option<String> {
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

pub(super) async fn optional_git_line(repository: &Path, args: &[&str]) -> Option<String> {
    let output = run_git(repository, args, 64 * 1024).await.ok()?;
    let value = String::from_utf8(output.bytes).ok()?.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

pub(super) async fn optional_git_lines(repository: &Path, args: &[&str]) -> Option<Vec<String>> {
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
