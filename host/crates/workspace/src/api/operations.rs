use super::*;
use aow_filesystem::FsError;
use aow_host_contract::workspace::*;
use serde_json::{Value, json};
use std::path::PathBuf;
fn validate_path(path: &std::path::Path) -> Result<PathBuf, HttpError> {
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(FsError::PathNotAbsolute(path.to_string_lossy().into_owned()).into());
    }
    Ok(path.into())
}
fn validate_name(value: &str) -> Result<String, HttpError> {
    let name = value.trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.len() > 255
        || name.contains(['/', '\\', '\0'])
    {
        return Err(FsError::InvalidFileName(name.into()).into());
    }
    Ok(name.into())
}
pub(super) async fn operation(
    Json(request): Json<WorkspaceRequest>,
) -> Result<Json<Value>, HttpError> {
    let value = match request {
        WorkspaceRequest::Home {} => {
            json!(PathResponse {
                path: std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"))
            })
        }
        WorkspaceRequest::TerminalDefaults {} => {
            let shell = std::env::var("SHELL")
                .ok()
                .filter(|shell| !shell.is_empty())
                .unwrap_or_else(|| {
                    if std::path::Path::new("/bin/bash").is_file() {
                        "/bin/bash".into()
                    } else {
                        "/bin/sh".into()
                    }
                });
            json!(TerminalDefaults { shell })
        }
        WorkspaceRequest::Account {} => {
            let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
            let mut result = std::ptr::null_mut();
            let mut buffer = vec![0_u8; 65536];
            let code = unsafe {
                libc::getpwuid_r(
                    libc::geteuid(),
                    account.as_mut_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    &mut result,
                )
            };
            if code != 0 || result.is_null() {
                return Err(HttpError::internal("Node account is unavailable"));
            }
            let name = unsafe { std::ffi::CStr::from_ptr((*result).pw_name) }
                .to_string_lossy()
                .into_owned();
            json!(Account { name })
        }
        WorkspaceRequest::TemporaryNote { root, extension } => {
            let root = validate_path(&root)?;
            let extension = extension.as_str();
            if !matches!(extension, "md" | "txt") {
                return Err(HttpError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_note_extension",
                    "Invalid note extension",
                    None,
                ));
            }
            let name = format!(".tmp-{}.{}", uuid::Uuid::new_v4(), extension);
            let file = root.join(&name);
            tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&file)
                .await?;
            json!(Note { path: file, name })
        }
        WorkspaceRequest::List { path } => {
            serde_json::to_value(aow_filesystem::list_directory(validate_path(&path)?).await?)
                .map_err(|e| HttpError::internal(e.to_string()))?
        }
        WorkspaceRequest::Read { path } => serde_json::to_value(
            aow_filesystem::read_text_file(
                validate_path(&path)?,
                aow_filesystem::DEFAULT_MAX_TEXT_BYTES,
            )
            .await?,
        )
        .map_err(|e| HttpError::internal(e.to_string()))?,
        WorkspaceRequest::Canonicalize { path } => {
            let canonical = tokio::fs::canonicalize(validate_path(&path)?).await?;
            json!(CanonicalPath {
                directory: tokio::fs::metadata(&canonical).await?.is_dir(),
                path: canonical
            })
        }
        WorkspaceRequest::Metadata { path, follow } => {
            let file = validate_path(&path)?;
            let metadata = if follow {
                tokio::fs::metadata(&file).await
            } else {
                tokio::fs::symlink_metadata(&file).await
            };
            match metadata {
                Ok(metadata) => {
                    json!(NodeMetadata {
                        exists: true,
                        file: metadata.is_file(),
                        directory: metadata.is_dir(),
                        symlink: metadata.is_symlink(),
                        size: metadata.len()
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    json!(NodeMetadata {
                        exists: false,
                        file: false,
                        directory: false,
                        symlink: false,
                        size: 0
                    })
                }
                Err(error) => return Err(error.into()),
            }
        }
        WorkspaceRequest::Create { parent, name, kind } => {
            let parent = validate_path(&parent)?;
            if !tokio::fs::metadata(&parent).await?.is_dir() {
                return Err(FsError::NotDirectory(parent.to_string_lossy().into_owned()).into());
            }
            let name = validate_name(&name)?;
            let file = parent.join(&name);

            let result = match kind {
                EntryKind::File => tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&file)
                    .await
                    .map(|_| ()),
                EntryKind::Directory => tokio::fs::create_dir(&file).await,
            };
            result.map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    HttpError::from(FsError::AlreadyExists(file.to_string_lossy().into_owned()))
                } else {
                    error.into()
                }
            })?;
            json!(CreatedEntry {
                path: file,
                name,
                kind
            })
        }
        WorkspaceRequest::MkdirAll { path } => {
            tokio::fs::create_dir_all(validate_path(&path)?).await?;
            json!(Empty {})
        }
        WorkspaceRequest::Rename {
            path,
            name,
            file_only,
        } => {
            let file = validate_path(&path)?;
            let name = validate_name(&name)?;
            let result = if file_only {
                aow_filesystem::rename_file(file, &name).await?
            } else {
                aow_filesystem::rename_entry(file, &name).await?
            };
            json!(aow_host_contract::workspace::RenameResult {
                path: result.to_string_lossy().into_owned(),
                name
            })
        }
        WorkspaceRequest::Delete { path } => {
            let file = validate_path(&path)?;
            if file.parent().is_none() {
                return Err(HttpError::new(
                    StatusCode::BAD_REQUEST,
                    "root_delete_forbidden",
                    "Cannot delete filesystem root",
                    None,
                ));
            }
            let metadata = tokio::fs::symlink_metadata(&file).await?;
            if metadata.is_symlink() || metadata.is_file() {
                tokio::fs::remove_file(file).await?;
            } else if metadata.is_dir() {
                tokio::fs::remove_dir_all(file).await?;
            } else {
                return Err(HttpError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_entry",
                    "Unsupported filesystem entry",
                    None,
                ));
            }
            json!(Empty {})
        }
        WorkspaceRequest::GitCommand {
            cwd,
            args: arguments,
        } => {
            let cwd = validate_path(&cwd)?;
            serde_json::to_value(crate::command(&cwd, &arguments).await.map_err(|e| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    "git_command_failed",
                    e.to_string(),
                    None,
                )
            })?)
            .unwrap()
        }
        WorkspaceRequest::GitStatus { repo } => {
            json!(crate::git::status(validate_path(&repo)?).await?)
        }
        WorkspaceRequest::GitRepositories { root, depth } => {
            json!(crate::git::discover_repositories(validate_path(&root)?, depth.min(8)).await?)
        }
        WorkspaceRequest::GitIgnored { root, paths } => {
            json!(crate::git::ignored_paths(validate_path(&root)?, paths).await?)
        }
        WorkspaceRequest::GitPull { repo } => {
            crate::git::pull(validate_path(&repo)?).await?;
            json!(Empty {})
        }
        WorkspaceRequest::GitPush { repo } => {
            crate::git::push(validate_path(&repo)?).await?;
            json!(Empty {})
        }
        WorkspaceRequest::GitLog { repo, limit } => {
            json!(crate::git::log(validate_path(&repo)?, limit).await?)
        }
        WorkspaceRequest::GitDiff { repo, path, staged } => {
            json!(crate::git::diff(validate_path(&repo)?, path.as_deref(), staged).await?)
        }
        WorkspaceRequest::GitCommitDetail { repo, commit } => {
            json!(crate::git::commit_detail(validate_path(&repo)?, &commit).await?)
        }
        WorkspaceRequest::GitCommitFiles { repo, commit } => {
            json!(crate::git::commit_files(validate_path(&repo)?, &commit).await?)
        }
        WorkspaceRequest::GitCommitDiff {
            repo,
            commit,
            path,
            original_path,
        } => json!(
            crate::git::commit_diff(
                validate_path(&repo)?,
                &commit,
                &path,
                original_path.as_deref()
            )
            .await?
        ),
    };
    Ok(Json(value))
}
