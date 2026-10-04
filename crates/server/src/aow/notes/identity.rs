use std::ffi::CStr;

use super::super::git::optional_git_output;
use super::*;

async fn repository_remote_identity(repository: &Path) -> Result<Option<PathBuf>, AowError> {
    let Some(remote) =
        optional_git_output(repository, &["config", "--get", "remote.origin.url"]).await?
    else {
        return Ok(None);
    };
    let remote = remote.trim();
    if remote.is_empty() {
        return Ok(None);
    }
    Ok(parse_remote_identity(remote).ok())
}

pub(in crate::aow) async fn default_notes_identity(repository: &Path) -> Result<PathBuf, AowError> {
    if let Some(identity) = repository_remote_identity(repository).await? {
        return Ok(identity);
    }
    if repository_has_remote(repository).await? {
        return Err(AowError::Invalid(
            "origin remote is required when Notes path is not provided".to_owned(),
        ));
    }
    local_repository_identity(repository, &process_account_name()?)
}

async fn repository_has_remote(repository: &Path) -> Result<bool, AowError> {
    Ok(!git_output(repository, &["remote"]).await?.trim().is_empty())
}

pub(in crate::aow) fn local_repository_identity(
    repository: &Path,
    account_name: &str,
) -> Result<PathBuf, AowError> {
    let repository_name = repository
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            AowError::Invalid("local Git repository name is not valid UTF-8".to_owned())
        })?;
    Ok(PathBuf::from("localhost")
        .join(validate_remote_component(account_name)?)
        .join(validate_remote_component(repository_name)?))
}

pub(in crate::aow) fn process_account_name() -> Result<String, AowError> {
    // SAFETY: The passwd record is copied into an owned String before returning.
    unsafe {
        let account = libc::getpwuid(libc::geteuid());
        if !account.is_null() && !(*account).pw_name.is_null() {
            let name = CStr::from_ptr((*account).pw_name)
                .to_string_lossy()
                .into_owned();
            if !name.is_empty() {
                return Ok(name);
            }
        }
    }
    Err(AowError::Invalid(
        "current account name is unavailable for local Notes mapping".to_owned(),
    ))
}

pub(in crate::aow) fn parse_remote_identity(remote: &str) -> Result<PathBuf, AowError> {
    let (authority, path) = if let Some((_, rest)) = remote.split_once("://") {
        if rest.starts_with('/') {
            return Err(AowError::Invalid(
                "origin remote must be an SSH or HTTP(S) repository URL".to_owned(),
            ));
        }
        let (authority, path) = rest.split_once('/').ok_or_else(|| {
            AowError::Invalid("origin remote URL does not include a repository path".to_owned())
        })?;
        (authority.rsplit('@').next().unwrap_or(authority), path)
    } else if let Some((authority, path)) = remote.split_once(':') {
        if authority.contains('/') || authority.contains('\\') {
            return Err(AowError::Invalid(
                "origin remote must be an SSH or HTTP(S) repository URL".to_owned(),
            ));
        }
        (authority.rsplit('@').next().unwrap_or(authority), path)
    } else {
        return Err(AowError::Invalid(
            "origin remote must be an SSH or HTTP(S) repository URL".to_owned(),
        ));
    };

    let authority = authority.trim();
    let authority = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map(|(host, _)| host).unwrap_or(rest)
    } else {
        authority
            .split_once(':')
            .map(|(host, _)| host)
            .unwrap_or(authority)
    }
    .to_ascii_lowercase();
    let path = path
        .split(['?', '#'])
        .next()
        .unwrap_or(path)
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or_else(|| {
            path.split(['?', '#'])
                .next()
                .unwrap_or(path)
                .trim_matches('/')
        });
    if authority.is_empty() || path.is_empty() {
        return Err(AowError::Invalid(
            "origin remote URL is incomplete".to_owned(),
        ));
    }

    let mut identity = PathBuf::from(validate_remote_component(&authority)?);
    let components = path.split('/').collect::<Vec<_>>();
    if components.len() < 2 {
        return Err(AowError::Invalid(
            "origin remote URL must include an owner and repository".to_owned(),
        ));
    }
    for component in components {
        identity.push(validate_remote_component(component)?);
    }
    Ok(identity)
}

fn validate_remote_component(value: &str) -> Result<&str, AowError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 255
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | '\0'))
    {
        return Err(AowError::Invalid(
            "origin remote URL contains an unsafe path component".to_owned(),
        ));
    }
    Ok(value)
}
