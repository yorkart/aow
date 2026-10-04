use std::{ffi::OsStr, path::Path};

pub const REPOSITORY_DIRECTORY: &str = "config-repo";
pub(super) const REGISTRIES: [&str; 3] =
    ["aow-projects.json", "aow-agents.json", "aow-settings.json"];
pub(super) fn is_configuration(path: &Path) -> bool {
    if REGISTRIES.iter().any(|name| path == Path::new(name))
        || path == Path::new("review-providers.json")
        || path == Path::new("inbox/labels.json")
    {
        return true;
    }
    let parts: Vec<_> = path.components().collect();
    if parts.len() == 5
        && parts[0].as_os_str() == "inbox"
        && parts[1].as_os_str() == "requirements"
        && matches!(parts[2].as_os_str().to_str(), Some("active" | "deleted"))
        && parts[3]
            .as_os_str()
            .to_str()
            .is_some_and(aow_id::is_valid_id)
        && matches!(
            parts[4].as_os_str().to_str(),
            Some("requirement.json" | "comments.jsonl")
        )
    {
        return true;
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
