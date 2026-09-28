use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use std::path::{Path, PathBuf};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};
use uuid::Uuid;

use aow_protocol::WriteResult;

use super::{
    FsError,
    metadata::version_for_metadata,
    paths::{absolute_path, display_path},
};

pub async fn write_atomic_stream<S, E>(
    path: impl AsRef<Path>,
    expected_version: Option<&str>,
    stream: S,
) -> Result<WriteResult, FsError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    write_stream(path, expected_version, false, stream).await
}

/// Publish a complete upload without replacing any existing entry, including
/// dangling symlinks. Concurrent uploads receive distinct names atomically.
pub async fn write_unique_stream<S, E>(
    path: impl AsRef<Path>,
    stream: S,
) -> Result<WriteResult, FsError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    write_stream(path, None, true, stream).await
}

async fn write_stream<S, E>(
    path: impl AsRef<Path>,
    expected_version: Option<&str>,
    keep_both: bool,
    mut stream: S,
) -> Result<WriteResult, FsError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    let path = absolute_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| FsError::PathNotAbsolute(display_path(&path)))?;
    let parent_metadata = fs::metadata(parent).await?;
    if !parent_metadata.is_dir() {
        return Err(FsError::NotDirectory(display_path(parent)));
    }

    let existing = match if keep_both {
        fs::symlink_metadata(&path).await
    } else {
        fs::metadata(&path).await
    } {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let existing_permissions = existing.as_ref().map(|metadata| metadata.permissions());
    if let Some(expected) = expected_version {
        ensure_expected_version(&path, expected, existing.as_ref())?;
    }

    let temp_path = parent.join(format!(".aow-{}.uploading", Uuid::new_v4()));
    let mut temp = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .await?;
    let mut size = 0_u64;
    let result = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| FsError::Stream(error.to_string()))?;
            temp.write_all(&chunk).await?;
            size = size.saturating_add(chunk.len() as u64);
        }
        temp.flush().await?;
        temp.sync_all().await?;
        if let Some(permissions) = existing_permissions.filter(|_| !keep_both) {
            temp.set_permissions(permissions).await?;
        }
        drop(temp);
        if let Some(expected) = expected_version {
            let current = match fs::metadata(&path).await {
                Ok(metadata) => Some(metadata),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            ensure_expected_version(&path, expected, current.as_ref())?;
        }
        let destination = if keep_both {
            publish_unique_upload(&temp_path, &path).await?
        } else {
            fs::rename(&temp_path, &path).await?;
            path.clone()
        };
        let metadata = fs::metadata(&destination).await?;
        Ok::<_, FsError>(WriteResult {
            path: display_path(&destination),
            size,
            created: keep_both || existing.is_none(),
            version: version_for_metadata(&metadata),
        })
    }
    .await;

    if result.is_err() {
        let _ = fs::remove_file(&temp_path).await;
    }
    result
}

async fn publish_unique_upload(temp_path: &Path, requested: &Path) -> Result<PathBuf, FsError> {
    let name = requested
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| FsError::InvalidFileName(display_path(requested)))?;
    let (stem, extension) = match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    };
    for index in 0..10_000 {
        let destination = if index == 0 {
            requested.to_path_buf()
        } else {
            let suffix = format!(" ({index}){extension}");
            let maximum = 255_usize
                .checked_sub(suffix.len())
                .ok_or_else(|| FsError::InvalidFileName(name.to_owned()))?;
            let mut end = stem.len().min(maximum);
            while !stem.is_char_boundary(end) {
                end -= 1;
            }
            requested.with_file_name(format!("{}{suffix}", &stem[..end]))
        };
        // A hard link publishes the completed temporary file atomically and
        // fails if *any* entry already occupies the destination. Both paths
        // live in the same directory/filesystem; no check-then-rename race.
        match fs::hard_link(temp_path, &destination).await {
            Ok(()) => {
                let _ = fs::remove_file(temp_path).await;
                return Ok(destination);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(FsError::AlreadyExists(display_path(requested)))
}

fn ensure_expected_version(
    path: &Path,
    expected: &str,
    metadata: Option<&std::fs::Metadata>,
) -> Result<(), FsError> {
    let current = metadata
        .map(version_for_metadata)
        .unwrap_or_else(|| "missing".to_owned());
    if expected == current {
        return Ok(());
    }
    Err(FsError::VersionConflict {
        path: display_path(path),
        expected: expected.to_owned(),
        current,
    })
}
