use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
};

use axum::http::StatusCode;
use ring::digest::{SHA256, digest};

use super::{Document, HttpError, MAX_BYTES, SaveRequest, Store};

fn invalid(message: &str) -> HttpError {
    HttpError::new(
        StatusCode::BAD_REQUEST,
        "invalid_server_environment",
        message,
        None,
    )
}

impl Store {
    pub(super) fn read(&self) -> Result<Document, HttpError> {
        // Never replace a symlink with a regular file when saving this editor.
        if fs::symlink_metadata(&self.path).is_ok_and(|metadata| !metadata.is_file()) {
            return Err(invalid("server.env 必须是普通文件，不能是符号链接或目录。"));
        }
        let (content, exists) = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
        {
            Ok(file) => {
                if !file.metadata()?.is_file() {
                    return Err(invalid("server.env 必须是普通文件。"));
                }
                let mut content = String::new();
                file.take((MAX_BYTES + 1) as u64)
                    .read_to_string(&mut content)?;
                validate(&content)?;
                (content, true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
            Err(error) => return Err(error.into()),
        };
        Ok(self.document(content, exists))
    }

    fn document(&self, content: String, exists: bool) -> Document {
        let hash = digest(&SHA256, content.as_bytes());
        let revision = format!("{exists}:{:x?}", hash.as_ref());
        Document {
            path: self.path.clone(),
            content,
            revision,
            exists,
            platform: std::env::consts::OS,
        }
    }

    pub(super) fn save(&self, request: SaveRequest) -> Result<Document, HttpError> {
        validate(&request.content)?;
        let _guard = self
            .operation
            .lock()
            .map_err(|_| HttpError::internal("server.env lock poisoned"))?;
        let current = self.read()?;
        if current.revision != request.revision {
            return Err(HttpError::new(
                StatusCode::CONFLICT,
                "server_environment_changed",
                "server.env 已被其他窗口或外部程序修改，请重新读取文件后再保存。当前草稿已保留。",
                None,
            ));
        }
        if current.exists && current.content == request.content {
            return Ok(current);
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| HttpError::internal("server.env has no parent"))?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        // NamedTempFile is created with 0600 permissions, including replacements
        // of existing files. It also cleans up failed writes automatically.
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(request.content.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.path)
            .map_err(|error| HttpError::from(error.error))?;
        File::open(parent)?.sync_all()?;
        Ok(self.document(request.content, true))
    }
}

fn validate(content: &str) -> Result<(), HttpError> {
    if content.len() > MAX_BYTES {
        return Err(invalid("server.env 不能超过 256 KiB。"));
    }
    if content.contains('\0') {
        return Err(invalid("server.env 不能包含空字符。"));
    }
    // This is a file editor: retain comments, quoting, duplicates and whitespace
    // verbatim, leaving environment syntax interpretation to the service loader.
    Ok(())
}
