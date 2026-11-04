use std::{
    io,
    path::{Path, PathBuf},
};

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use tokio::fs;

use super::*;
use crate::paths::display_path;

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("aow-test-{}", aow_id::new_id()));
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
