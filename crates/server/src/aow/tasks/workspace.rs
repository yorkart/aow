use super::*;
use std::path::Component;

/// Paperclip's sanitizeSlugPart rules: lowercase ASCII letters/digits, underscores,
/// collapsed hyphens, and no leading/trailing separators. Bound the suffix while
/// preserving the task identity already present in the branch template.
pub(super) fn normalize_slug(value: Option<&str>) -> Option<String> {
    let mut slug = String::new();
    for character in value.unwrap_or_default().trim().chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches(['-', '_']);
    let slug = &slug[..slug.len().min(64)];
    let slug = slug.trim_end_matches(['-', '_']);
    (!slug.is_empty()).then(|| slug.to_owned())
}

pub(super) fn task_slug(prompt: &str, supplied: Option<&str>) -> Option<String> {
    // Task has no separate title yet. Use its first non-empty line as the title.
    // An explicitly empty suffix keeps the ID-only branch behavior available.
    normalize_slug(supplied.or_else(|| prompt.lines().map(str::trim).find(|line| !line.is_empty())))
}

pub(super) fn task_branch(
    template: &str,
    id: &str,
    slug: Option<&str>,
) -> Result<String, AowError> {
    if !template.contains("{{task_id}}") {
        return Err(AowError::Invalid(
            "Task branch template must include {{task_id}}".into(),
        ));
    }
    let template = if slug.is_none() {
        template.replace("-{{slug}}", "")
    } else {
        template.to_owned()
    };
    let branch = template
        .replace("{{task_id}}", id)
        .replace("{{slug}}", slug.unwrap_or_default());
    if branch.contains("{{") || branch.contains("}}") {
        return Err(AowError::Invalid(
            "Unknown task branch template placeholder".into(),
        ));
    }
    Ok(branch)
}

/// Deterministic identities let a retry reuse a checkout created before a failed ACP preparation.
pub(super) async fn create_task_workspace(
    state: &AppState,
    project_id: &str,
    id: &str,
    slug: Option<&str>,
) -> Result<String, AowError> {
    let project = state.aow.project(project_id).await?;
    let main = project
        .worktrees
        .iter()
        .find(|item| item.is_main)
        .ok_or_else(|| AowError::Invalid("Project has no main checkout".into()))?;
    let policy = &project.task_worktree;
    let branch = task_branch(&policy.branch_template, id, slug)?;
    let configured = Path::new(&policy.worktree_parent_dir);
    if configured.as_os_str().is_empty() {
        return Err(AowError::Invalid(
            "Worktree parent directory is empty".into(),
        ));
    }
    let joined = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        Path::new(&main.path).join(configured)
    };
    let mut parent = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                parent.pop();
            }
            Component::CurDir => {}
            part => parent.push(part.as_os_str()),
        }
    }
    if project
        .worktrees
        .iter()
        .any(|item| parent.starts_with(&item.path))
    {
        return Err(AowError::Invalid(
            "Task worktrees must be outside repository checkouts".into(),
        ));
    }
    tokio::fs::create_dir_all(&parent).await?;
    let parent = paths::canonical_directory(&parent).await?;
    if project
        .worktrees
        .iter()
        .any(|item| parent.starts_with(&item.path))
    {
        return Err(AowError::Invalid(
            "Task worktree parent resolves inside a repository checkout".into(),
        ));
    }
    let project_parent = parent.join(&project.id);
    tokio::fs::create_dir_all(&project_parent).await?;
    let project_parent = paths::canonical_directory(&project_parent).await?;
    if !project_parent.starts_with(&parent) {
        return Err(AowError::Invalid(
            "Project worktree directory resolves outside its managed parent".into(),
        ));
    }
    let target = project_parent.join(id);
    if let Some(existing) = project
        .worktrees
        .iter()
        .find(|item| Path::new(&item.path) == target && item.branch == branch)
    {
        return Ok(existing.path.clone());
    }
    let base_ref = match policy
        .base_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.to_owned(),
        None => {
            let branches = state.aow.project_branches(project_id).await?;
            if branches.default_branch.is_empty() {
                "HEAD".into()
            } else {
                branches.default_branch
            }
        }
    };
    let created = state
        .aow
        .create_worktree(
            project_id,
            CreateWorktreeRequest {
                branch,
                base_ref,
                path: target.to_string_lossy().into_owned(),
                pull_first: false,
            },
        )
        .await?;
    state.workspace_events.projects_changed();
    Ok(created.worktree.path)
}
