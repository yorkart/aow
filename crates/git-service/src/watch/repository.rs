use std::path::{Path, PathBuf};

use crate::command::{SMALL_OUTPUT_LIMIT, run_git};

async fn metadata_output(root: &Path, args: &[&str]) -> Option<String> {
    // Reuse the service's timeout, output limits and GIT_OPTIONAL_LOCKS policy.
    let output = run_git(root, args, SMALL_OUTPUT_LIMIT).await.ok()?;
    if output.truncated {
        return None;
    }
    String::from_utf8(output.bytes).ok()
}

#[derive(Clone)]
pub(super) struct Repository {
    pub(super) root: PathBuf,
    pub(super) git: PathBuf,
    pub(super) common: PathBuf,
    pub(super) main: PathBuf,
}

impl Repository {
    pub(super) async fn resolve(root: &Path) -> Option<Self> {
        let text = metadata_output(root, &["rev-parse", "--git-dir", "--git-common-dir"]).await?;
        let mut lines = text.lines();
        let resolve = |line: &str| std::fs::canonicalize(root.join(line)).ok();
        let git = resolve(lines.next()?)?;
        let common = resolve(lines.next()?)?;
        // The first entry is the main checkout, also when the supplied root
        // is a linked worktree or uses --separate-git-dir.
        let listing = metadata_output(root, &["worktree", "list", "--porcelain", "-z"]).await?;
        let main = PathBuf::from(listing.split('\0').next()?.strip_prefix("worktree ")?);
        Some(Self {
            root: root.to_owned(),
            git,
            common,
            main,
        })
    }

    pub(super) fn affected_by(&self, path: &Path) -> bool {
        [&self.git, &self.common].iter().any(|directory| {
            let Ok(relative) = path.strip_prefix(directory) else {
                return false;
            };
            let parts: Vec<_> = relative.iter().filter_map(|part| part.to_str()).collect();
            match parts.as_slice() {
                [] => true,
                [
                    "HEAD" | "config" | "config.worktree" | "packed-refs" | "shallow" | "commondir",
                ] => true,
                ["refs" | "reftable", ..] => !path.to_string_lossy().ends_with(".lock"),
                ["worktrees"] | ["worktrees", _] => true,
                [
                    "worktrees",
                    _,
                    "HEAD" | "gitdir" | "commondir" | "locked" | "config.worktree",
                ] => true,
                ["worktrees", _, "refs" | "reftable", ..] => {
                    !path.to_string_lossy().ends_with(".lock")
                }
                _ => false,
            }
        })
    }
}
