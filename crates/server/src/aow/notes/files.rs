use tokio::fs::OpenOptions;

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

pub(in crate::aow) async fn prepare_notes_directory(path: &Path) -> Result<String, AowError> {
    if !path.is_absolute() {
        return Err(AowError::Invalid(
            "Notes directory must be an absolute path".to_owned(),
        ));
    }
    tokio::fs::create_dir_all(path).await?;
    Ok(paths::canonical_directory(path)
        .await?
        .to_string_lossy()
        .into_owned())
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
    paths::canonical_directory(Path::new(&path)).await
}

pub(in crate::aow) async fn create_temporary_note_file(
    root: &Path,
    extension: &str,
    mut next_id: impl FnMut() -> String,
) -> Result<(PathBuf, String), AowError> {
    for _ in 0..5 {
        let name = format!(".tmp-{}.{}", next_id(), extension);
        let path = root.join(&name);
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
        {
            Ok(_) => return Ok((path, name)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(AowError::Invalid(
        "could not allocate a unique temporary note name after 5 attempts".to_owned(),
    ))
}
