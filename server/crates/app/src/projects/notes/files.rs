use super::*;

#[derive(Debug, Deserialize)]
pub(in crate::projects) struct CreateTemporaryNoteRequest {
    pub(in crate::projects) extension: TemporaryNoteExtension,
}

#[derive(Debug, Serialize)]
pub(in crate::projects) struct TemporaryNoteResult {
    pub(in crate::projects) path: String,
    pub(in crate::projects) name: String,
    pub(in crate::projects) kind: &'static str,
}

pub(in crate::projects) async fn notes_root_canonical(
    manager: &ProjectService,
    id: &str,
) -> Result<PathBuf, ServiceError> {
    let path = manager
        .lock()?
        .projects
        .iter()
        .find(|project| project.id == id)
        .ok_or_else(|| ServiceError::ProjectNotFound(id.to_owned()))?
        .notes_path
        .clone();
    manager.node.canonical_directory(Path::new(&path)).await
}
