use super::*;

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct CreateTemporaryNoteRequest {
    pub(in crate::aow) extension: TemporaryNoteExtension,
}

#[derive(Debug, Serialize)]
pub(in crate::aow) struct TemporaryNoteResult {
    pub(in crate::aow) path: String,
    pub(in crate::aow) name: String,
    pub(in crate::aow) kind: &'static str,
}

pub(in crate::aow) async fn notes_root_canonical(
    manager: &AowManager,
    id: &str,
) -> Result<PathBuf, AowError> {
    let path = manager
        .lock()?
        .projects
        .iter()
        .find(|project| project.id == id)
        .ok_or_else(|| AowError::ProjectNotFound(id.to_owned()))?
        .notes_path
        .clone();
    manager.canonical_directory(Path::new(&path)).await
}
