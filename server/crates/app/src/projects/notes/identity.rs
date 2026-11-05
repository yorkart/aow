use super::*;

async fn repository_remote_identity(
    manager: &ProjectService,
    repository: &Path,
) -> Result<Option<PathBuf>, ServiceError> {
    let Some(remote) = manager
        .node
        .optional_git_output(repository, &["config", "--get", "remote.origin.url"])
        .await?
    else {
        return Ok(None);
    };
    let remote = remote.trim();
    if remote.is_empty() {
        return Ok(None);
    }
    Ok(parse_remote_identity(remote).ok())
}

pub(crate) async fn default_notes_identity(
    manager: &ProjectService,
    repository: &Path,
) -> Result<PathBuf, ServiceError> {
    if let Some(identity) = repository_remote_identity(manager, repository).await? {
        return Ok(identity);
    }
    if repository_has_remote(manager, repository).await? {
        return Err(ServiceError::Invalid(
            aow_i18n::text("validation.origin_remote_is_required_when_notes_path_is_not_provided")
                .to_owned(),
        ));
    }
    local_repository_identity(repository, &manager.node.node_account().await?)
}

async fn repository_has_remote(
    manager: &ProjectService,
    repository: &Path,
) -> Result<bool, ServiceError> {
    Ok(!manager
        .node
        .git_output(repository, &["remote"])
        .await?
        .trim()
        .is_empty())
}

pub(crate) fn local_repository_identity(
    repository: &Path,
    account_name: &str,
) -> Result<PathBuf, ServiceError> {
    let repository_name = repository
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            ServiceError::Invalid(
                aow_i18n::text("validation.local_git_repository_name_is_not_valid_utf_8")
                    .to_owned(),
            )
        })?;
    Ok(PathBuf::from("localhost")
        .join(validate_remote_component(account_name)?)
        .join(validate_remote_component(repository_name)?))
}

pub(crate) fn parse_remote_identity(remote: &str) -> Result<PathBuf, ServiceError> {
    let (authority, path) = if let Some((_, rest)) = remote.split_once("://") {
        if rest.starts_with('/') {
            return Err(ServiceError::Invalid(
                aow_i18n::text("validation.origin_remote_must_be_an_ssh_or_http_s_repository")
                    .to_owned(),
            ));
        }
        let (authority, path) = rest.split_once('/').ok_or_else(|| {
            ServiceError::Invalid(
                aow_i18n::text("validation.origin_remote_url_does_not_include_a_repository_path")
                    .to_owned(),
            )
        })?;
        (authority.rsplit('@').next().unwrap_or(authority), path)
    } else if let Some((authority, path)) = remote.split_once(':') {
        if authority.contains('/') || authority.contains('\\') {
            return Err(ServiceError::Invalid(
                aow_i18n::text("validation.origin_remote_must_be_an_ssh_or_http_s_repository")
                    .to_owned(),
            ));
        }
        (authority.rsplit('@').next().unwrap_or(authority), path)
    } else {
        return Err(ServiceError::Invalid(
            aow_i18n::text("validation.origin_remote_must_be_an_ssh_or_http_s_repository")
                .to_owned(),
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
        return Err(ServiceError::Invalid(
            aow_i18n::text("validation.origin_remote_url_is_incomplete").to_owned(),
        ));
    }

    let mut identity = PathBuf::from(validate_remote_component(&authority)?);
    let components = path.split('/').collect::<Vec<_>>();
    if components.len() < 2 {
        return Err(ServiceError::Invalid(
            aow_i18n::text("validation.origin_remote_url_must_include_an_owner_and_repository")
                .to_owned(),
        ));
    }
    for component in components {
        identity.push(validate_remote_component(component)?);
    }
    Ok(identity)
}

fn validate_remote_component(value: &str) -> Result<&str, ServiceError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 255
        || value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | '\0'))
    {
        return Err(ServiceError::Invalid(
            aow_i18n::text("validation.origin_remote_url_contains_an_unsafe_path_component")
                .to_owned(),
        ));
    }
    Ok(value)
}
