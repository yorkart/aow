use serde_json::Value;
use std::{collections::HashSet, path::Path};

use super::{GitPlatformError, Result, SCRIPT_LIMIT, Settings};

// Validate clickable links without making assumptions about the hosting platform.
pub(super) fn web_link(result: &Value, field: &str) -> Result<Option<String>> {
    match result.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let url = reqwest::Url::parse(value).map_err(invalid_json)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || value
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
            {
                return Err(invalid_json(format!(
                    "{field} must be an HTTP(S) URL without credentials"
                )));
            }
            Ok(Some(value.clone()))
        }
        _ => Err(invalid_json(format!(
            "{field} must be a URL string or null"
        ))),
    }
}
pub(super) fn valid_filename(name: &str) -> bool {
    name.strip_suffix(".mjs").is_some_and(|stem| {
        !stem.is_empty()
            && name.len() <= 100
            && stem.as_bytes()[0].is_ascii_alphanumeric()
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    })
}
pub(super) fn validate_settings(settings: &Settings) -> Result<()> {
    if settings.providers.len() > 32 {
        return Err(GitPlatformError::Invalid(
            aow_i18n::text("server.at_most_32_providers_are_allowed").into(),
        ));
    }
    let mut ids = HashSet::new();
    let mut hosts = HashSet::new();
    for p in &settings.providers {
        if !valid_filename(&p.id) || !ids.insert(p.id.to_ascii_lowercase()) {
            return Err(GitPlatformError::Invalid(
                aow_i18n::text("server.provider_filenames_must_be_unique_mjs").into(),
            ));
        }
        if p.name.trim().is_empty() || p.script.trim().is_empty() || p.script.len() > SCRIPT_LIMIT {
            return Err(GitPlatformError::Invalid(
                aow_i18n::text("server.name_and_script_are_required_script_limit_is_1").into(),
            ));
        }
        if p.hosts.is_empty() {
            return Err(GitPlatformError::Invalid(
                aow_i18n::text("server.configure_at_least_one_remote_domain").into(),
            ));
        }
        for host in &p.hosts {
            if host.is_empty()
                || host.len() > 253
                || !host
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-_:[]".contains(&c))
            {
                return Err(GitPlatformError::Invalid(aow_i18n::message(
                    "server.invalid_remote_domain",
                    &[("p0", host.to_string())],
                )));
            }
            if !hosts.insert(host) {
                return Err(GitPlatformError::Invalid(aow_i18n::message(
                    "server.multiple_providers_match_the_same_domain",
                    &[("p0", host.to_string())],
                )));
            }
        }
    }
    Ok(())
}
// Parse HTTPS, ssh:// and scp-style remotes. Local paths have no provider.
pub(super) fn parse_remote(raw: &str) -> Option<(String, String)> {
    let (host, path) = if raw.contains("://") {
        let url = reqwest::Url::parse(raw).ok()?;
        if !matches!(url.scheme(), "https" | "http" | "ssh" | "git") {
            return None;
        }
        let mut host = url.host_str()?.to_ascii_lowercase();
        if let Some(port) = url
            .port()
            .filter(|_| matches!(url.scheme(), "https" | "http"))
        {
            host.push_str(&format!(":{port}"));
        }
        (host, url.path().trim_start_matches('/').to_string())
    } else {
        let (authority, path) = raw.split_once(':')?;
        if authority.contains('/') || authority.is_empty() {
            return None;
        }
        (
            authority.rsplit('@').next()?.to_ascii_lowercase(),
            path.trim_start_matches('/').to_string(),
        )
    };
    let path = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_end_matches('/'))
        .to_string();
    if host.is_empty() || path.is_empty() || !path.contains('/') {
        return None;
    }
    Some((host, path))
}
pub(super) fn absolute(repo: &str) -> Result<()> {
    if !Path::new(repo).is_absolute() {
        return Err(GitPlatformError::Invalid(
            aow_i18n::text("errors.repository_path_must_be_absolute").into(),
        ));
    }
    Ok(())
}
pub(super) fn invalid_json(message: impl ToString) -> GitPlatformError {
    GitPlatformError::InvalidJson(message.to_string())
}
