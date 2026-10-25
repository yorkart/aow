use crate::git_platform_providers::GitPlatformError;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::{Component, Path};
type Result<T> = std::result::Result<T, GitPlatformError>;
fn invalid_json(message: impl ToString) -> GitPlatformError {
    GitPlatformError::InvalidJson(message.to_string())
}

pub(crate) fn positive(number: u64) -> Result<()> {
    if number == 0 {
        Err(GitPlatformError::Invalid(
            aow_i18n::text("validation.pr_number_must_be_positive").into(),
        ))
    } else {
        Ok(())
    }
}
pub(crate) fn safe_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(GitPlatformError::Invalid(
            aow_i18n::text("validation.file_path_must_be_repository_relative").into(),
        ));
    }
    Ok(())
}
pub(crate) fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(invalid_json)
}
