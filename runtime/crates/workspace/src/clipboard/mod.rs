//! Temporary images referenced by terminal input must live on the execution node.
mod image;
use crate::api::error::HttpError;
use axum::{Json, body::Body, http::StatusCode};
use futures_util::StreamExt;
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tokio::io::AsyncWriteExt;
const IMAGE_LIMIT: u64 = 10 * 1024 * 1024;
const QUOTA: u64 = 256 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) struct Clipboard {
    directory: PathBuf,
    operation: tokio::sync::Mutex<()>,
}
impl Clipboard {
    pub(crate) fn open(root: &Path) -> anyhow::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let directory = root.join("cache/clipboard-images");
        anyhow::ensure!(
            !directory
                .ancestors()
                .any(|path| std::fs::symlink_metadata(path)
                    .is_ok_and(|metadata| metadata.is_symlink())),
            "Clipboard directory cannot contain symbolic links"
        );
        std::fs::create_dir_all(&directory)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        let value = Self {
            directory: directory.canonicalize()?,
            operation: Default::default(),
        };
        value.clean()?;
        Ok(value)
    }
    pub(crate) fn clean(&self) -> std::io::Result<u64> {
        let mut used = 0_u64;
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            if metadata
                .modified()
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age >= TTL)
            {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            } else {
                used = used.saturating_add(metadata.len());
            }
        }
        Ok(used)
    }
    pub(crate) async fn upload(&self, body: Body) -> Result<Json<serde_json::Value>, HttpError> {
        let _operation = self.operation.lock().await;
        let used = self.clean()?;
        let id = uuid::Uuid::new_v4();
        let temporary = self.directory.join(format!(".{id}.tmp"));
        let _cleanup = Temporary(temporary.clone());
        let result=async {
            let mut options=tokio::fs::OpenOptions::new();options.create_new(true).write(true).mode(0o600);
            let mut file=options.open(&temporary).await?;let mut bytes=Vec::new();let mut stream=body.into_data_stream();
            while let Some(chunk)=stream.next().await {
                let chunk=chunk.map_err(|error|HttpError::new(StatusCode::BAD_REQUEST,"request_stream_failed",error.to_string(),None))?;
                if (bytes.len() as u64).saturating_add(chunk.len() as u64)>IMAGE_LIMIT{return Err(HttpError::new(StatusCode::PAYLOAD_TOO_LARGE,"clipboard_image_too_large","Clipboard image exceeds 10 MiB",None));}
                if used.saturating_add(bytes.len() as u64).saturating_add(chunk.len() as u64)>QUOTA{return Err(HttpError::new(StatusCode::INSUFFICIENT_STORAGE,"clipboard_quota_exceeded","Clipboard storage quota exceeded",None));}
                bytes.extend_from_slice(&chunk);file.write_all(&chunk).await?;
            }
            let kind=image::clipboard_image_type(&bytes).ok_or_else(||HttpError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE,"unsupported_clipboard_image","Unsupported clipboard image",None))?;
            file.sync_all().await?;drop(file);
            let path=self.directory.join(format!("{id}.{}",kind.extension));tokio::fs::rename(&temporary,&path).await?;
            let expires_at=(chrono::Utc::now()+chrono::Duration::from_std(TTL).unwrap()).to_rfc3339_opts(chrono::SecondsFormat::Millis,true);
            Ok(Json(serde_json::json!({"path":path,"mime":kind.mime,"size":bytes.len(),"expires_at":expires_at})))
        }.await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(temporary).await;
        }
        result
    }
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
