use std::path::Path;
use tokio::fs::{self, File};

use aow_protocol::TextFile;

use super::{
    FsError,
    metadata::version_for_metadata,
    paths::{absolute_path, display_path},
};

pub async fn read_text_file(path: impl AsRef<Path>, maximum: u64) -> Result<TextFile, FsError> {
    let path = absolute_path(path)?;
    let metadata = fs::metadata(&path).await?;
    if !metadata.is_file() {
        return Err(FsError::NotFile(display_path(&path)));
    }
    if metadata.len() > maximum {
        return Err(FsError::TooLarge {
            path: display_path(&path),
            size: metadata.len(),
            maximum,
        });
    }
    let bytes = fs::read(&path).await?;
    if bytes.contains(&0) {
        return Err(FsError::Binary(display_path(&path)));
    }
    let content = String::from_utf8(bytes).map_err(|_| FsError::NotUtf8(display_path(&path)))?;
    Ok(TextFile {
        path: display_path(&path),
        size: metadata.len(),
        version: version_for_metadata(&metadata),
        language: language_for_path(&path).to_owned(),
        mime: mime_guess::from_path(&path)
            .first_or_octet_stream()
            .essence_str()
            .to_owned(),
        content,
    })
}

pub async fn open_file(path: impl AsRef<Path>) -> Result<(File, std::fs::Metadata), FsError> {
    let path = absolute_path(path)?;
    let file = File::open(&path).await?;
    let metadata = file.metadata().await?;
    if !metadata.is_file() {
        return Err(FsError::NotFile(display_path(&path)));
    }
    Ok((file, metadata))
}
fn language_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "json" => "json",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "html" => "html",
        "css" => "css",
        "sh" | "bash" => "shell",
        _ => "plaintext",
    }
}
