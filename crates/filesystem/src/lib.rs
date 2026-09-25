#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{ffi::CString, os::unix::ffi::OsStrExt};
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use aow_protocol::{DirectoryListing, FileEntry, FileKind, TextFile, WriteResult};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use thiserror::Error;
use tokio::{
    fs::{self, File, OpenOptions},
    io::AsyncWriteExt,
};
use uuid::Uuid;

/// Default local AoW state, shared by the server and command-line tools.
pub fn default_state_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("AOW_STATE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("aow");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".local").join("state").join("aow");
    }
    std::env::temp_dir().join(format!("aow-{}", std::process::id()))
}

pub const DEFAULT_MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("path must be absolute: {0}")]
    PathNotAbsolute(String),
    #[error("path is not a directory: {0}")]
    NotDirectory(String),
    #[error("path is not a regular file: {0}")]
    NotFile(String),
    #[error("file is too large to edit ({size} bytes, maximum {maximum}): {path}")]
    TooLarge {
        path: String,
        size: u64,
        maximum: u64,
    },
    #[error("file is not UTF-8 text: {0}")]
    NotUtf8(String),
    #[error("file contains NUL bytes: {0}")]
    Binary(String),
    #[error("file changed on disk (expected {expected}, current {current}): {path}")]
    VersionConflict {
        path: String,
        expected: String,
        current: String,
    },
    #[error("invalid file name: {0}")]
    InvalidFileName(String),
    #[error("destination already exists: {0}")]
    AlreadyExists(String),
    #[error("streaming request failed: {0}")]
    Stream(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn absolute_path(raw: impl AsRef<Path>) -> Result<PathBuf, FsError> {
    let path = raw.as_ref();
    if !path.is_absolute() {
        return Err(FsError::PathNotAbsolute(
            path.to_string_lossy().into_owned(),
        ));
    }
    Ok(path.to_path_buf())
}

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

pub async fn rename_file(
    source: impl AsRef<Path>,
    requested_name: &str,
) -> Result<PathBuf, FsError> {
    let source = absolute_path(source)?;
    let metadata = fs::symlink_metadata(&source).await?;
    if !metadata.file_type().is_file() {
        return Err(FsError::NotFile(display_path(&source)));
    }
    rename_entry_path(source, requested_name).await
}

pub async fn rename_entry(
    source: impl AsRef<Path>,
    requested_name: &str,
) -> Result<PathBuf, FsError> {
    let source = absolute_path(source)?;
    let metadata = fs::symlink_metadata(&source).await?;
    if !metadata.file_type().is_file() && !metadata.file_type().is_dir() {
        return Err(FsError::NotFile(display_path(&source)));
    }
    rename_entry_path(source, requested_name).await
}

async fn rename_entry_path(source: PathBuf, requested_name: &str) -> Result<PathBuf, FsError> {
    let name = requested_name.trim();
    let name_path = Path::new(name);
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 255
        || name.contains(['/', '\\', '\0'])
        || name_path.file_name().and_then(|value| value.to_str()) != Some(name)
    {
        return Err(FsError::InvalidFileName(requested_name.to_owned()));
    }

    let parent = source
        .parent()
        .ok_or_else(|| FsError::PathNotAbsolute(display_path(&source)))?;
    let destination = parent.join(name);
    if destination == source {
        return Ok(destination);
    }

    rename_no_replace(&source, &destination)
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                FsError::AlreadyExists(display_path(&destination))
            } else {
                FsError::Io(error)
            }
        })?;
    Ok(destination)
}

#[cfg(target_os = "linux")]
async fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    // musl toolchains do not consistently export a renameat2 wrapper. Use the
    // Linux syscall directly to retain atomic no-replace behavior with either libc.
    // SAFETY: Both pointers are valid NUL-terminated paths for the duration of the call.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
async fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL"))?;
    // Darwin handles case-only renames of the same directory entry while
    // atomically rejecting other existing entries, including hard links.
    // Do not fall back to check-then-rename on volumes without RENAME_EXCL.
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
async fn rename_no_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(destination).await {
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "destination already exists",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(source, destination).await
}

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

pub fn version_for_metadata(metadata: &std::fs::Metadata) -> String {
    version_for_metadata_impl(metadata)
}

#[cfg(unix)]
fn version_for_metadata_impl(metadata: &std::fs::Metadata) -> String {
    format!(
        "{:x}-{:x}-{:x}-{:x}-{:x}-{:x}",
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime_nsec(),
    )
}

#[cfg(not(unix))]
fn version_for_metadata_impl(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:x}-{:x}", metadata.len(), modified)
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

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use std::io;

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("aow-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[tokio::test]
    async fn directories_are_sorted_before_files() {
        let root = temp_dir();
        fs::create_dir(root.join("z-dir")).await.unwrap();
        fs::write(root.join("a-file.txt"), b"hello").await.unwrap();
        let listing = list_directory(&root).await.unwrap();
        assert_eq!(listing.entries[0].name, "z-dir");
        assert_eq!(listing.entries[1].name, "a-file.txt");
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn unique_uploads_preserve_existing_files_and_concurrent_contents() {
        let root = temp_dir();
        let path = root.join("报告.txt");
        fs::write(&path, b"original").await.unwrap();
        fs::create_dir(root.join("报告 (1).txt")).await.unwrap();
        let (first, second) = tokio::join!(
            write_unique_stream(
                &path,
                stream::iter([Ok::<_, io::Error>(Bytes::from_static(b"first"))])
            ),
            write_unique_stream(
                &path,
                stream::iter([Ok::<_, io::Error>(Bytes::from_static(b"second"))])
            ),
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_ne!(first.path, second.path);
        assert!(first.created && second.created);
        assert_eq!(fs::read(&path).await.unwrap(), b"original");
        assert_eq!(fs::read(&first.path).await.unwrap(), b"first");
        assert_eq!(fs::read(&second.path).await.unwrap(), b"second");
        assert!(root.join("报告 (2).txt").exists());
        assert!(root.join("报告 (3).txt").exists());
        assert_eq!(list_directory(&root).await.unwrap().entries.len(), 4);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn unique_uploads_preserve_long_unicode_names_and_clean_failed_streams() {
        let root = temp_dir();
        let path = root.join(format!("{}.png", "图".repeat(83)));
        fs::write(&path, b"original").await.unwrap();
        let result = write_unique_stream(&path, stream::iter([Ok::<_, io::Error>(Bytes::new())]))
            .await
            .unwrap();
        assert!(result.path.ends_with(" (1).png"));
        assert!(Path::new(&result.path).file_name().unwrap().len() <= 255);
        assert_eq!(fs::read(&result.path).await.unwrap().len(), 0);
        let failed_path = root.join("failed.bin");
        let chunks = stream::iter([
            Ok::<_, io::Error>(Bytes::from_static(b"partial")),
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "cancelled")),
        ]);
        assert!(write_unique_stream(&failed_path, chunks).await.is_err());
        assert_eq!(list_directory(&root).await.unwrap().entries.len(), 2);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unique_uploads_do_not_follow_or_replace_dangling_symlinks() {
        let root = temp_dir();
        let path = root.join("image.png");
        let target = root.join("missing.png");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let result = write_unique_stream(
            &path,
            stream::iter([Ok::<_, io::Error>(Bytes::from_static(b"image"))]),
        )
        .await
        .unwrap();
        assert_eq!(result.path, display_path(&root.join("image (1).png")));
        assert!(fs::symlink_metadata(&path).await.unwrap().is_symlink());
        assert!(!target.exists());
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_checks_version() {
        let root = temp_dir();
        let path = root.join("file.txt");
        fs::write(&path, b"one").await.unwrap();
        let current = fs::metadata(&path).await.unwrap();
        let version = version_for_metadata(&current);
        let chunks = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from_static(b"two"))]);
        let result = write_atomic_stream(&path, Some(&version), chunks)
            .await
            .unwrap();
        assert!(!result.created);
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "two");

        let stale = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from_static(b"bad"))]);
        assert!(matches!(
            write_atomic_stream(&path, Some(&version), stale).await,
            Err(FsError::VersionConflict { .. })
        ));
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn atomic_write_rechecks_version_before_replace() {
        let root = temp_dir();
        let path = root.join("file.txt");
        fs::write(&path, b"one").await.unwrap();
        let version = version_for_metadata(&fs::metadata(&path).await.unwrap());
        let externally_modified = path.clone();
        let chunks = stream::iter([Bytes::from_static(b"two")]).map(move |chunk| {
            std::fs::write(&externally_modified, b"external").unwrap();
            Ok::<_, std::io::Error>(chunk)
        });

        assert!(matches!(
            write_atomic_stream(&path, Some(&version), chunks).await,
            Err(FsError::VersionConflict { .. })
        ));
        assert_eq!(fs::read_to_string(&path).await.unwrap(), "external");
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".uploading")
        }));
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn interrupted_upload_removes_temporary_file_and_destination() {
        let root = temp_dir();
        let path = root.join("interrupted.bin");
        let chunks = stream::iter(vec![
            Ok::<_, io::Error>(Bytes::from_static(b"partial")),
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "cancelled")),
        ]);
        assert!(matches!(
            write_atomic_stream(&path, None, chunks).await,
            Err(FsError::Stream(_))
        ));
        assert!(!path.exists());
        let entries = std::fs::read_dir(&root)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(entries.is_empty(), "temporary upload file was not removed");
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn rename_file_keeps_content_and_rejects_existing_destination() {
        let root = temp_dir();
        let source = root.join("before.txt");
        let existing = root.join("existing.txt");
        fs::write(&source, b"hello").await.unwrap();
        fs::write(&existing, b"keep").await.unwrap();

        let renamed = rename_file(&source, "after.md").await.unwrap();
        assert_eq!(renamed, root.join("after.md"));
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(&renamed).await.unwrap(), "hello");

        assert!(matches!(
            rename_file(&renamed, "existing.txt").await,
            Err(FsError::AlreadyExists(_))
        ));
        assert_eq!(fs::read_to_string(&renamed).await.unwrap(), "hello");
        assert_eq!(fs::read_to_string(&existing).await.unwrap(), "keep");
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn rename_file_rejects_paths_and_non_files() {
        let root = temp_dir();
        let source = root.join("source.txt");
        fs::write(&source, b"hello").await.unwrap();

        let too_long = "x".repeat(256);
        for invalid in [
            "",
            ".",
            "..",
            "nested/file.txt",
            "nested\\file.txt",
            &too_long,
        ] {
            assert!(matches!(
                rename_file(&source, invalid).await,
                Err(FsError::InvalidFileName(_))
            ));
        }
        assert!(matches!(
            rename_file(&root, "renamed").await,
            Err(FsError::NotFile(_))
        ));
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_renames_never_replace_the_winning_entry() {
        let root = temp_dir();
        fs::create_dir_all(&root).await.unwrap();
        for directory in [false, true] {
            for attempt in 0..50 {
                let a = root.join(format!("a-{directory}-{attempt}"));
                let b = root.join(format!("b-{directory}-{attempt}"));
                let target = format!("target-{directory}-{attempt}");
                if directory {
                    fs::create_dir(&a).await.unwrap();
                    fs::create_dir(&b).await.unwrap();
                } else {
                    fs::write(&a, b"first").await.unwrap();
                    fs::write(&b, b"second").await.unwrap();
                }
                let (first, second) =
                    tokio::join!(rename_entry(&a, &target), rename_entry(&b, &target));
                assert_ne!(
                    first.is_ok(),
                    second.is_ok(),
                    "exactly one rename must succeed"
                );
                let (loser, error, winning_content) = match first {
                    Ok(_) => (&b, second.unwrap_err(), "first"),
                    Err(error) => (&a, error, "second"),
                };
                assert!(matches!(error, FsError::AlreadyExists(_)));
                assert!(loser.exists(), "the rejected source must remain intact");
                if !directory {
                    assert_eq!(
                        fs::read_to_string(root.join(&target)).await.unwrap(),
                        winning_content
                    );
                }
            }
        }
        fs::remove_dir_all(root).await.unwrap();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_case_only_renames_preserve_files_and_directories() {
        let root = temp_dir();
        fs::create_dir_all(&root).await.unwrap();
        fs::write(root.join("Readme.md"), b"original")
            .await
            .unwrap();
        rename_file(root.join("Readme.md"), "README.md")
            .await
            .unwrap();
        fs::create_dir(root.join("Sources")).await.unwrap();
        fs::write(root.join("Sources/kept"), b"nested")
            .await
            .unwrap();
        rename_entry(root.join("Sources"), "SOURCES").await.unwrap();
        let names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(names.contains(&std::ffi::OsString::from("README.md")));
        assert!(names.contains(&std::ffi::OsString::from("SOURCES")));
        assert_eq!(fs::read(root.join("README.md")).await.unwrap(), b"original");
        assert_eq!(
            fs::read(root.join("SOURCES/kept")).await.unwrap(),
            b"nested"
        );
        fs::remove_dir_all(root).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rename_rejects_existing_hardlinks_and_dangling_symlinks() {
        let root = temp_dir();
        fs::create_dir_all(&root).await.unwrap();
        let source = root.join("source");
        fs::write(&source, b"original").await.unwrap();
        fs::hard_link(&source, root.join("hardlink")).await.unwrap();
        std::os::unix::fs::symlink("missing", root.join("dangling")).unwrap();
        for target in ["hardlink", "dangling"] {
            assert!(matches!(
                rename_file(&source, target).await,
                Err(FsError::AlreadyExists(_))
            ));
        }
        assert_eq!(fs::read(&source).await.unwrap(), b"original");
        assert_eq!(
            fs::read_link(root.join("dangling")).await.unwrap(),
            PathBuf::from("missing")
        );
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn rename_entry_supports_directories_without_overwriting() {
        let root = temp_dir();
        let source = root.join("before");
        let existing = root.join("existing");
        fs::create_dir(&source).await.unwrap();
        fs::create_dir(&existing).await.unwrap();
        fs::write(source.join("file.txt"), b"hello").await.unwrap();

        let renamed = rename_entry(&source, "after").await.unwrap();
        assert_eq!(renamed, root.join("after"));
        assert_eq!(
            fs::read_to_string(renamed.join("file.txt")).await.unwrap(),
            "hello"
        );
        assert!(matches!(
            rename_entry(&renamed, "existing").await,
            Err(FsError::AlreadyExists(_))
        ));
        fs::remove_dir_all(root).await.unwrap();
    }
}
