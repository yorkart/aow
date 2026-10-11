use super::progress::step;
use super::*;
use crate::aow::worktrees::{CreateWorktreeResponse, parse_worktrees};

fn validate_git_value(label: &str, value: &str) -> Result<String, AowError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(AowError::Invalid(format!(
            "{label} must contain 1-1024 printable characters"
        )));
    }
    Ok(value.to_owned())
}

pub(super) fn validate_request(request: &mut CreateWorktreeRequest) -> Result<(), AowError> {
    request.branch = validate_git_value("branch", &request.branch)?;
    request.base_ref = validate_git_value("base ref", &request.base_ref)?;
    request.path = request.path.trim().into();
    let path = Path::new(&request.path);
    if !path.is_absolute()
        || request.path.len() > 4096
        || request.path.chars().any(char::is_control)
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(AowError::Invalid(
            "worktree path must be an absolute, normalized path".into(),
        ));
    }
    Ok(())
}

impl AowManager {
    #[cfg(test)]
    pub(in crate::aow) async fn create_worktree(
        &self,
        id: &str,
        request: CreateWorktreeRequest,
    ) -> Result<CreateWorktreeResponse, AowError> {
        self.create_worktree_steps(id, request, None).await
    }

    pub(super) async fn create_worktree_steps(
        &self,
        id: &str,
        mut request: CreateWorktreeRequest,
        progress: Option<&Progress>,
    ) -> Result<CreateWorktreeResponse, AowError> {
        let project_lock = self.inner.removals.project_lock(id)?;
        let _project = step(progress, 0, async { Ok(project_lock.lock().await) }).await?;
        let (stored, path) = step(progress, 1, async {
            validate_request(&mut request)?;
            let stored = self
                .lock()?
                .projects
                .iter()
                .find(|project| project.id == id)
                .cloned()
                .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?;
            let path = paths::validate_new_worktree_path(&request.path).await?;
            let cwd = Path::new(&stored.registered_path);
            git_output(cwd, &["check-ref-format", "--branch", &request.branch]).await?;
            git_output(
                cwd,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{}^{{commit}}", request.base_ref),
                ],
            )
            .await?;
            if super::super::git::optional_git_output(
                cwd,
                &[
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{}", request.branch),
                ],
            )
            .await?
            .is_some()
            {
                return Err(AowError::Git(format!(
                    "branch already exists: {}",
                    request.branch
                )));
            }
            Ok((stored, path))
        })
        .await?;
        let cwd = Path::new(&stored.registered_path);
        if request.pull_first {
            step(progress, 2, async {
                let output = git_output(cwd, &["worktree", "list", "--porcelain"]).await?;
                let main = parse_worktrees(id, &output).into_iter().find(|worktree| worktree.is_main)
                    .ok_or_else(|| AowError::Git("project has no main worktree".into()))?;
                git_output(Path::new(&main.path), &["pull"]).await.map_err(|error| {
                    let message = format!("git pull failed in main worktree {}: {error}\n可先在终端检查 git pull，或取消勾选「创建前更新主仓库」后重试。", main.path);
                    if matches!(error, AowError::Timeout(_)) { AowError::Timeout(message) } else { AowError::Git(message) }
                })?;
                Ok(())
            }).await?;
        } else if let Some(progress) = progress {
            progress.skip(2);
        }

        step(progress, 3, async {
            git_output(
                cwd,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &request.branch,
                    "--",
                    &path.to_string_lossy(),
                    &request.base_ref,
                ],
            )
            .await?;
            Ok(())
        })
        .await?;
        step(progress, 4, async {
            let created_path = paths::canonical_directory(&path).await?;
            let project = projects::project_from_stored_strict(stored).await?;
            let worktree = project
                .worktrees
                .iter()
                .find(|worktree| Path::new(&worktree.path) == created_path)
                .cloned()
                .ok_or_else(|| {
                    AowError::Git(format!(
                        "created worktree was not returned by git worktree list: {}",
                        created_path.display()
                    ))
                })?;
            Ok(CreateWorktreeResponse { project, worktree })
        })
        .await
    }
}
