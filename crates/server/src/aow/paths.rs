use super::*;

pub(super) async fn validate_absolute_directory(value: &str) -> Result<PathBuf, AowError> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(AowError::Invalid(
            aow_i18n::text("validation.project_path_must_be_absolute").to_owned(),
        ));
    }
    canonical_directory(path).await
}

pub(super) async fn validate_new_worktree_path(value: &str) -> Result<PathBuf, AowError> {
    let value = value.trim();
    let path = PathBuf::from(value);
    if value.is_empty() || value.len() > 4096 || !path.is_absolute() || value.contains('\0') {
        return Err(AowError::Invalid(
            aow_i18n::text("validation.worktree_path_must_be_an_absolute_path").to_owned(),
        ));
    }
    if tokio::fs::try_exists(&path).await? {
        return Err(AowError::Invalid(format!(
            "worktree path already exists: {}",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        AowError::Invalid(
            aow_i18n::text("validation.worktree_path_must_have_a_parent_directory").to_owned(),
        )
    })?;
    let parent = canonical_directory(parent).await?;
    let name = path.file_name().ok_or_else(|| {
        AowError::Invalid(
            aow_i18n::text("validation.worktree_path_must_include_a_directory_name").to_owned(),
        )
    })?;
    Ok(parent.join(name))
}

pub(super) async fn canonical_directory(path: &Path) -> Result<PathBuf, AowError> {
    let canonical = tokio::fs::canonicalize(path).await?;
    if !tokio::fs::metadata(&canonical).await?.is_dir() {
        return Err(AowError::Invalid(format!(
            "path is not a directory: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}
