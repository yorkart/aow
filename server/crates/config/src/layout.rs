use std::{ffi::OsStr, path::Path};

pub(super) const REGISTRIES: [&str; 4] = [
    "aow-projects.json",
    "aow-agents.json",
    "aow-settings.json",
    "aow-agent-roles.json",
];
pub(super) fn is_configuration(path: &Path) -> bool {
    if REGISTRIES.iter().any(|name| path == Path::new(name))
        || path == Path::new("git-platform-providers.json")
        || path == Path::new("aow-acp.json")
    {
        return true;
    }
    if path.parent() == Some(Path::new("git-platform-providers"))
        && path.extension().is_some_and(|ext| ext == "mjs")
    {
        return path.file_stem().and_then(OsStr::to_str).is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 96
                && id.as_bytes()[0].is_ascii_alphanumeric()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        });
    }
    path.parent() == Some(Path::new("automations/tasks"))
        && path.extension().is_some_and(|ext| ext == "json")
        && path.file_stem().and_then(OsStr::to_str).is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 96
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
}
