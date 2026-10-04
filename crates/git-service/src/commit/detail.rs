use std::{collections::BTreeSet, path::Path};

use aow_protocol::{GitCommitDetail, GitCommitStats, GitIdentity};

use crate::{
    GitError,
    command::{SMALL_OUTPUT_LIMIT, absolute, run_git},
    history::{optional_git_line, preferred_remote, upstream_snapshot},
};

use super::common::{first_parent, validate_object_id};

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

pub(crate) fn parse_numstat(bytes: &[u8]) -> Result<GitCommitStats, GitError> {
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
