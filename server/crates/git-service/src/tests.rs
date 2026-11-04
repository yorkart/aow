use std::{path::Path, process::Command as StdCommand};

use aow_protocol::GitIgnoredPaths;
use tempfile::TempDir;

use super::*;
use crate::{command::LimitedOutput, commit::parse_numstat, status::parse_status};

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
        parse_numstat(b"3\t1\ttext.txt\0-\t-\tbinary.bin\x002\t0\t\0old.txt\0new.txt\0").unwrap();
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
    git(&repo, &["config", "user.name", "AoW Test"]);
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
    git(&repo, &["config", "user.name", "AoW Test"]);
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
    git(&repo, &["config", "user.name", "AoW Test"]);
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
