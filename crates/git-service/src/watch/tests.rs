use super::*;

async fn git(root: &Path, args: &[&str]) -> String {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Watch Test",
            "-c",
            "user.email=watch@example.invalid",
        ])
        .args(args)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

async fn repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "-b", "main"]).await;
    git(
        directory.path(),
        &["commit", "--allow-empty", "-m", "initial"],
    )
    .await;
    directory
}

async fn changed(
    receiver: &mut watch::Receiver<GitChanges>,
    root: &Path,
    after: u64,
) -> GitChanges {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = receiver.borrow_and_update().clone();
            if snapshot
                .repositories
                .get(root)
                .is_some_and(|revision| *revision > after)
            {
                return snapshot;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .expect("native Git metadata event was not delivered")
}

#[tokio::test]
async fn metadata_tracks_history_but_ignores_working_files_index_objects_and_locks() {
    let directory = repository().await;
    let root = directory.path();
    let repository = Repository::resolve(root).await.unwrap();
    let before = collect(&repository, &mut Watches::new());
    std::fs::write(root.join("untracked"), "hello").unwrap();
    std::fs::write(repository.git.join("index.lock"), "lock").unwrap();
    std::fs::write(repository.git.join("objects/irrelevant"), "object").unwrap();
    assert_eq!(before, collect(&repository, &mut Watches::new()));
    std::fs::remove_file(repository.git.join("index.lock")).unwrap();
    git(root, &["add", "untracked"]).await;
    assert_eq!(before, collect(&repository, &mut Watches::new()));
    let head = std::fs::read(repository.git.join("HEAD")).unwrap();
    git(root, &["commit", "-m", "next"]).await;
    assert_eq!(
        head,
        std::fs::read(repository.git.join("HEAD")).unwrap(),
        "symbolic HEAD does not change on commit"
    );
    let after = collect(&repository, &mut Watches::new());
    assert_ne!(before, after);
    assert_eq!(
        before.worktrees, after.worktrees,
        "ordinary commits do not reload the project list"
    );
    for relative in [
        "index",
        "index.lock",
        "logs/HEAD",
        "objects/irrelevant",
        "refs/heads/main.lock",
        "worktrees/linked/index",
    ] {
        assert!(
            !repository.affected_by(&repository.common.join(relative)),
            "{relative}"
        );
    }
    assert!(!repository.affected_by(&root.join("untracked")));
    git(root, &["pack-refs", "--all"]).await;
    assert_ne!(after, collect(&repository, &mut Watches::new()));
}

#[tokio::test]
async fn native_events_discover_external_worktrees_moves_locks_and_removals() {
    let directory = repository().await;
    let root = directory.path();
    let outside = tempfile::tempdir().unwrap();
    let worktree = outside.path().join("feature");
    let moved = outside.path().join("moved");
    let watcher = GitWatcher::new(vec![root.to_owned()]).unwrap();
    let mut receiver = watcher.subscribe();
    let mut revision = changed(&mut receiver, root, 0).await.revision;
    // No worktrees directory exists when the watch is first attached.
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    )
    .await;
    revision = changed(&mut receiver, root, revision).await.revision;
    git(
        root,
        &[
            "worktree",
            "move",
            worktree.to_str().unwrap(),
            moved.to_str().unwrap(),
        ],
    )
    .await;
    revision = changed(&mut receiver, root, revision).await.revision;
    git(root, &["worktree", "lock", moved.to_str().unwrap()]).await;
    revision = changed(&mut receiver, root, revision).await.revision;
    git(root, &["worktree", "unlock", moved.to_str().unwrap()]).await;
    revision = changed(&mut receiver, root, revision).await.revision;
    git(root, &["worktree", "remove", moved.to_str().unwrap()]).await;
    let after = changed(&mut receiver, root, revision).await;
    assert!(!after.repositories.contains_key(&moved));
    assert!(after.worktrees > revision);
}

#[tokio::test]
async fn linked_worktrees_share_ref_changes_and_reconnects_receive_current_versions() {
    let directory = repository().await;
    let root = directory.path();
    let outside = tempfile::tempdir().unwrap();
    let linked = outside.path().join("linked");
    git(
        root,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    )
    .await;
    // A linked checkout also watches the main checkout and shared refs.
    let watcher = GitWatcher::new(vec![linked.clone()]).unwrap();
    let mut receiver = watcher.subscribe();
    let before = changed(&mut receiver, &linked, 0).await;
    git(
        &linked,
        &["commit", "--allow-empty", "-m", "external commit"],
    )
    .await;
    let after = changed(&mut receiver, &linked, before.revision).await;
    assert!(after.repositories[root] > before.revision);
    assert_eq!(before.worktrees, after.worktrees);
    git(&linked, &["checkout", "--detach"]).await;
    changed(&mut receiver, &linked, after.revision).await;
    let reconnected = watcher.subscribe();
    assert!(reconnected.borrow().revision > before.revision);
    let revision = reconnected.borrow().revision;
    drop(receiver);
    drop(reconnected);
    git(
        &linked,
        &["commit", "--allow-empty", "-m", "without clients"],
    )
    .await;
    let mut reconnected = watcher.subscribe();
    changed(&mut reconnected, &linked, revision).await;
}

#[tokio::test]
async fn replacing_roots_clears_removed_versions_and_dropping_watcher_closes_updates() {
    let directory = repository().await;
    let watcher = GitWatcher::new(vec![directory.path().to_owned()]).unwrap();
    let mut receiver = watcher.subscribe();
    let before = changed(&mut receiver, directory.path(), 0).await;
    watcher.set_repositories(vec![]).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let next = receiver.borrow_and_update().clone();
            if next.repositories.is_empty() && next.worktrees > before.worktrees {
                break;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    drop(watcher);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), receiver.changed())
            .await
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn reconciliation_detects_deleted_worktree_directory_without_git_events() {
    let directory = repository().await;
    let root = directory.path();
    let outside = tempfile::tempdir().unwrap();
    let linked = outside.path().join("linked");
    git(
        root,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    )
    .await;
    let repository = Repository::resolve(root).await.unwrap();
    let before = collect(&repository, &mut Watches::new());
    std::fs::remove_dir_all(linked).unwrap();
    assert_ne!(before, collect(&repository, &mut Watches::new()));
    assert!(
        git(root, &["worktree", "list", "--porcelain"])
            .await
            .contains("prunable")
    );
}

#[tokio::test]
async fn shared_remote_refs_config_and_shallow_boundaries_invalidate_history() {
    let directory = repository().await;
    let root = directory.path();
    let repository = Repository::resolve(root).await.unwrap();
    let watcher = GitWatcher::new(vec![root.to_owned()]).unwrap();
    let mut receiver = watcher.subscribe();
    let mut revision = changed(&mut receiver, root, 0).await.revision;
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]).await;
    revision = changed(&mut receiver, root, revision).await.revision;
    git(root, &["config", "branch.main.remote", "origin"]).await;
    revision = changed(&mut receiver, root, revision).await.revision;
    git(root, &["pack-refs", "--all"]).await;
    revision = changed(&mut receiver, root, revision).await.revision;
    std::fs::write(
        repository.common.join("shallow"),
        git(root, &["rev-parse", "HEAD"]).await,
    )
    .unwrap();
    changed(&mut receiver, root, revision).await;
}
