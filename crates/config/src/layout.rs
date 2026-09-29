use std::{ffi::OsStr, path::Path};

pub const REPOSITORY_DIRECTORY: &str = "config-repo";
pub(super) const REGISTRIES: [&str; 3] =
    ["aow-projects.json", "aow-agents.json", "aow-settings.json"];
pub(super) fn is_configuration(path: &Path) -> bool {
    if REGISTRIES.iter().any(|name| path == Path::new(name))
        || path == Path::new("review-providers.json")
        || path == Path::new("tasks/statuses.json")
        || path == Path::new("tasks/sources.json")
    {
        return true;
    }
    if let Ok(relative) = path.strip_prefix("tasks/inbox") {
        let parts: Vec<_> = relative.iter().collect();
        return parts.len() == 2
            && parts[0].to_str().is_some_and(task_id)
            && path.extension().is_some_and(|ext| ext == "json")
            && path
                .file_stem()
                .and_then(OsStr::to_str)
                .is_some_and(task_id);
    }
    ((path.parent() == Some(Path::new("automations/tasks"))
        && path.extension().is_some_and(|ext| ext == "json"))
        || (path.parent() == Some(Path::new("review-providers"))
            && path.extension().is_some_and(|ext| ext == "py")))
        && path.file_stem().and_then(OsStr::to_str).is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 96
                && id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
}

fn task_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
