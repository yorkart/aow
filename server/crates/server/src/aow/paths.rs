use super::*;
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
