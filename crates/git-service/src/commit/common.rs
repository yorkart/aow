use std::path::Path;

use crate::{GitError, command::run_git};

pub(super) fn validate_object_id(value: &str) -> Result<(), GitError> {
    if (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitError::Command("invalid commit id".to_owned()))
    }
}

pub(super) async fn first_parent(repository: &Path, commit: &str) -> Option<String> {
    let output = run_git(
        repository,
        &["rev-parse", &format!("{commit}^1")],
        64 * 1024,
    )
    .await
    .ok()?;
    Some(String::from_utf8_lossy(&output.bytes).trim().to_owned())
}
