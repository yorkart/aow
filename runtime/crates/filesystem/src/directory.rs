#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{path::Path, time::UNIX_EPOCH};
use tokio::fs;

use aow_protocol::{DirectoryListing, FileEntry, FileKind};

use super::{
    FsError,
    paths::{absolute_path, display_path},
};

pub async fn list_directory(path: impl AsRef<Path>) -> Result<DirectoryListing, FsError> {
    let path = absolute_path(path)?;
    let metadata = fs::metadata(&path).await?;
    if !metadata.is_dir() {
        return Err(FsError::NotDirectory(display_path(&path)));
    }

    let mut reader = fs::read_dir(&path).await?;
    let mut entries = Vec::new();
    while let Some(entry) = reader.next_entry().await? {
        let entry_path = entry.path();
        let symlink_metadata = fs::symlink_metadata(&entry_path).await?;
        let file_type = symlink_metadata.file_type();
        let followed_metadata = if file_type.is_symlink() {
            fs::metadata(&entry_path).await.ok()
        } else {
            None
        };
        let kind =
            if followed_metadata.as_ref().is_some_and(|item| item.is_dir()) || file_type.is_dir() {
                FileKind::Directory
            } else if followed_metadata
                .as_ref()
                .is_some_and(|item| item.is_file())
                || file_type.is_file()
            {
                FileKind::File
            } else if file_type.is_symlink() {
                FileKind::Symlink
            } else {
                FileKind::Other
            };
        let metadata = followed_metadata.as_ref().unwrap_or(&symlink_metadata);
        let name = entry.file_name().to_string_lossy().into_owned();
        let link_target = if file_type.is_symlink() {
            fs::read_link(&entry_path)
                .await
                .ok()
                .map(|value| value.to_string_lossy().into_owned())
        } else {
            None
        };
        entries.push(FileEntry {
            hidden: name.starts_with('.'),
            name,
            path: display_path(&entry_path),
            kind,
            size: metadata.len(),
            modified_ms: modified_ms(metadata),
            readonly: metadata.permissions().readonly(),
            mode: metadata_mode(metadata),
            links: metadata_links(metadata),
            uid: metadata_uid(metadata),
            gid: metadata_gid(metadata),
            is_symlink: file_type.is_symlink(),
            link_target,
        });
    }

    entries.sort_by(|left, right| {
        kind_rank(&left.kind)
            .cmp(&kind_rank(&right.kind))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(DirectoryListing {
        path: display_path(&path),
        entries,
    })
}
fn modified_ms(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

#[cfg(unix)]
fn metadata_mode(metadata: &std::fs::Metadata) -> u32 {
    metadata.mode()
}
#[cfg(not(unix))]
fn metadata_mode(_: &std::fs::Metadata) -> u32 {
    0
}
#[cfg(unix)]
fn metadata_links(metadata: &std::fs::Metadata) -> u64 {
    metadata.nlink()
}
#[cfg(not(unix))]
fn metadata_links(_: &std::fs::Metadata) -> u64 {
    1
}
#[cfg(unix)]
fn metadata_uid(metadata: &std::fs::Metadata) -> u32 {
    metadata.uid()
}
#[cfg(not(unix))]
fn metadata_uid(_: &std::fs::Metadata) -> u32 {
    0
}
#[cfg(unix)]
fn metadata_gid(metadata: &std::fs::Metadata) -> u32 {
    metadata.gid()
}
#[cfg(not(unix))]
fn metadata_gid(_: &std::fs::Metadata) -> u32 {
    0
}
fn kind_rank(kind: &FileKind) -> u8 {
    match kind {
        FileKind::Directory => 0,
        FileKind::File => 1,
        FileKind::Symlink => 2,
        FileKind::Other => 3,
    }
}
