use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Component, Path},
};

use super::{PullRequestError, Result, SCRIPT_LIMIT, Settings};

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
pub(super) fn managed_script_path(path: &str) -> bool {
    let path = Path::new(path);
    path.parent() == Some(Path::new("review-providers"))
        && path.extension().is_some_and(|s| s == "py")
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(valid_id)
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub(super) fn validate_settings(settings: &Settings) -> Result<()> {
    if settings.providers.len() > 32 {
        return Err(PullRequestError::Invalid(
            "最多配置 32 个 Provider。".into(),
        ));
    }
    let mut ids = HashSet::new();
    let mut hosts = HashSet::new();
    for p in &settings.providers {
        if !valid_id(&p.id) || !ids.insert(&p.id) || p.id == "auto" {
            return Err(PullRequestError::Invalid(
                "Provider ID 必须唯一，只能包含字母、数字、-、_，且不能为 auto。".into(),
            ));
        }
        if p.name.trim().is_empty() || p.script.trim().is_empty() || p.script.len() > SCRIPT_LIMIT {
            return Err(PullRequestError::Invalid(
                "名称和脚本必填；脚本最大 1 MiB。".into(),
            ));
        }
        if p.hosts.is_empty() {
            return Err(PullRequestError::Invalid(
                "请至少配置一个 remote 域名。".into(),
            ));
        }
        for host in &p.hosts {
            if host.is_empty()
                || host.len() > 253
                || !host
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-_:[]".contains(&c))
            {
                return Err(PullRequestError::Invalid(format!(
                    "无效的 remote 域名：{host}"
                )));
            }
            if p.enabled && !hosts.insert(host) {
                return Err(PullRequestError::Invalid(format!(
                    "多个启用的 Provider 匹配同一个域名：{host}"
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
        return Err(PullRequestError::Invalid(
            "repository path must be absolute".into(),
        ));
    }
    Ok(())
}
pub(super) fn positive(number: u64) -> Result<()> {
    if number == 0 {
        Err(PullRequestError::Invalid(
            "PR number must be positive".into(),
        ))
    } else {
        Ok(())
    }
}
pub(super) fn safe_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(PullRequestError::Invalid(
            "file path must be repository-relative".into(),
        ));
    }
    Ok(())
}
pub(super) fn invalid_json(message: impl ToString) -> PullRequestError {
    PullRequestError::InvalidJson(message.to_string())
}
pub(super) fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(invalid_json)
}
