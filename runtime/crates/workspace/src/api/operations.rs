use super::*;
use aow_filesystem::FsError;
use serde_json::{Value, json};
use std::path::PathBuf;
#[derive(Deserialize)]
pub(super) struct Request {
    operation: String,
    #[serde(default)]
    arguments: Value,
}
fn path(value: &Value, key: &str) -> Result<PathBuf, HttpError> {
    let path = PathBuf::from(string(value, key)?);
    validate_path(&path)?;
    Ok(path)
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, HttpError> {
    value[key].as_str().ok_or_else(|| {
        HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_workspace_request",
            format!("Missing {key}"),
            None,
        )
    })
}
fn validate_path(path: &std::path::Path) -> Result<(), HttpError> {
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
    Ok(())
}
fn name(value: &Value) -> Result<String, HttpError> {
    let name = string(value, "name")?.trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.len() > 255
        || name.contains(['/', '\\', '\0'])
    {
        return Err(FsError::InvalidFileName(name.into()).into());
    }
    Ok(name.into())
}
pub(super) async fn operation(Json(request): Json<Request>) -> Result<Json<Value>, HttpError> {
    let args = request.arguments;
    let value = match request.operation.as_str() {
        "home" => {
            json!({"path":std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(||PathBuf::from("/"))})
        }
        "terminal_defaults" => {
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
            json!({"shell":shell})
        }
        "account" => {
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
            json!({"name":name})
        }
        "temporary_note" => {
            let root = path(&args, "root")?;
            let extension = string(&args, "extension")?;
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
            json!({"path":file,"name":name})
        }
        "list" => serde_json::to_value(aow_filesystem::list_directory(path(&args, "path")?).await?)
            .map_err(|e| HttpError::internal(e.to_string()))?,
        "read" => serde_json::to_value(
            aow_filesystem::read_text_file(
                path(&args, "path")?,
                aow_filesystem::DEFAULT_MAX_TEXT_BYTES,
            )
            .await?,
        )
        .map_err(|e| HttpError::internal(e.to_string()))?,
        "canonicalize" => {
            let canonical = tokio::fs::canonicalize(path(&args, "path")?).await?;
            json!({"path":canonical,"directory":tokio::fs::metadata(&canonical).await?.is_dir()})
        }
        "metadata" => {
            let file = path(&args, "path")?;
            let metadata = if args["follow"].as_bool().unwrap_or(true) {
                tokio::fs::metadata(&file).await
            } else {
                tokio::fs::symlink_metadata(&file).await
            };
            match metadata {
                Ok(metadata) => {
                    json!(aow_runtime_contract::NodeMetadata {
                        exists: true,
                        file: metadata.is_file(),
                        directory: metadata.is_dir(),
                        symlink: metadata.is_symlink(),
                        size: metadata.len()
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    json!(aow_runtime_contract::NodeMetadata::default())
                }
                Err(error) => return Err(error.into()),
            }
        }
        "create" => {
            let parent = path(&args, "parent")?;
            if !tokio::fs::metadata(&parent).await?.is_dir() {
                return Err(FsError::NotDirectory(parent.to_string_lossy().into_owned()).into());
            }
            let name = name(&args)?;
            let file = parent.join(&name);
            let kind = string(&args, "kind")?;
            let result = match kind {
                "file" => tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&file)
                    .await
                    .map(|_| ()),
                "directory" => tokio::fs::create_dir(&file).await,
                _ => {
                    return Err(HttpError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_entry_kind",
                        "Invalid file kind",
                        None,
                    ));
                }
            };
            result.map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    HttpError::from(FsError::AlreadyExists(file.to_string_lossy().into_owned()))
                } else {
                    error.into()
                }
            })?;
            json!({"path":file,"name":name,"kind":kind})
        }
        "mkdir_all" => {
            tokio::fs::create_dir_all(path(&args, "path")?).await?;
            json!({})
        }
        "rename" => {
            let file = path(&args, "path")?;
            let name = name(&args)?;
            let result = if args["file_only"].as_bool().unwrap_or(false) {
                aow_filesystem::rename_file(file, &name).await?
            } else {
                aow_filesystem::rename_entry(file, &name).await?
            };
            json!({"path":result,"name":name})
        }
        "delete" => {
            let file = path(&args, "path")?;
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
            json!({})
        }
        "git_command" => {
            let cwd = path(&args, "cwd")?;
            let arguments: Vec<String> = serde_json::from_value(args["args"].clone())
                .map_err(|e| HttpError::internal(e.to_string()))?;
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
        "git_status" => json!(crate::git::status(path(&args, "repo")?).await?),
        "git_repositories" => json!(
            crate::git::discover_repositories(
                path(&args, "root")?,
                args["depth"].as_u64().unwrap_or(4).min(8) as usize
            )
            .await?
        ),
        "git_ignored" => json!(
            crate::git::ignored_paths(
                path(&args, "root")?,
                serde_json::from_value::<Vec<PathBuf>>(args["paths"].clone())
                    .map_err(|e| HttpError::internal(e.to_string()))?
            )
            .await?
        ),
        "git_pull" => {
            crate::git::pull(path(&args, "repo")?).await?;
            json!({})
        }
        "git_push" => {
            crate::git::push(path(&args, "repo")?).await?;
            json!({})
        }
        "git_log" => json!(
            crate::git::log(
                path(&args, "repo")?,
                args["limit"].as_u64().unwrap_or(100) as usize
            )
            .await?
        ),
        "git_diff" => json!(
            crate::git::diff(
                path(&args, "repo")?,
                args["path"].as_str(),
                args["staged"].as_bool().unwrap_or(false)
            )
            .await?
        ),
        "git_commit_detail" => {
            json!(crate::git::commit_detail(path(&args, "repo")?, string(&args, "commit")?).await?)
        }
        "git_commit_files" => {
            json!(crate::git::commit_files(path(&args, "repo")?, string(&args, "commit")?).await?)
        }
        "git_commit_diff" => json!(
            crate::git::commit_diff(
                path(&args, "repo")?,
                string(&args, "commit")?,
                string(&args, "path")?,
                args["original_path"].as_str()
            )
            .await?
        ),
        _ => {
            return Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "invalid_workspace_operation",
                "Unsupported workspace operation",
                None,
            ));
        }
    };
    Ok(Json(value))
}
