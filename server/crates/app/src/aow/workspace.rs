//! Project business workflows call node capabilities through the SDK.
use super::*;
pub(crate) fn node_error(error: aow_runtime_client::RuntimeError) -> AowError {
    match error {
        aow_runtime_client::RuntimeError::HttpStatus {
            status, message, ..
        } if status == StatusCode::NOT_FOUND => {
            AowError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, message))
        }
        aow_runtime_client::RuntimeError::HttpStatus {
            status, message, ..
        } if status == StatusCode::FORBIDDEN => AowError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            message,
        )),
        error => AowError::Invalid(error.to_string()),
    }
}
impl AowManager {
    pub(crate) fn node(&self) -> Option<&aow_runtime_client::RuntimeClient> {
        self.inner.runtime_client.as_ref()
    }
    pub(super) async fn git_output(&self, cwd: &Path, args: &[&str]) -> Result<String, AowError> {
        if let Some(runtime) = self.node() {
            let output = runtime.workspace_git(cwd, args).await.map_err(node_error)?;
            if !output.success {
                return Err(AowError::Git(
                    String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                ));
            }
            return String::from_utf8(output.stdout).map_err(|e| AowError::Git(e.to_string()));
        }
        #[cfg(test)]
        {
            git::git_output(cwd, args).await
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Workspace runtime is unavailable".into()))
        }
    }
    pub(super) async fn optional_git_output(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> Result<Option<String>, AowError> {
        if let Some(runtime) = self.node() {
            let output = runtime.workspace_git(cwd, args).await.map_err(node_error)?;
            if output.success {
                return String::from_utf8(output.stdout)
                    .map(Some)
                    .map_err(|e| AowError::Git(e.to_string()));
            }
            if output.code == Some(1) && output.stderr.is_empty() {
                return Ok(None);
            }
            return Err(AowError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        #[cfg(test)]
        {
            git::optional_git_output(cwd, args).await
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Workspace runtime is unavailable".into()))
        }
    }
    pub(crate) async fn canonical_directory(&self, path: &Path) -> Result<PathBuf, AowError> {
        if let Some(runtime) = self.node() {
            let (path, directory) = runtime
                .workspace_canonicalize(path)
                .await
                .map_err(node_error)?;
            if !directory {
                return Err(AowError::Invalid("Node path is not a directory".into()));
            }
            return Ok(path);
        }
        #[cfg(test)]
        {
            paths::canonical_directory(path).await
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Workspace runtime is unavailable".into()))
        }
    }
    pub(super) async fn node_metadata(
        &self,
        path: &Path,
        follow: bool,
    ) -> Result<aow_runtime_client::NodeMetadata, AowError> {
        if let Some(runtime) = self.node() {
            return runtime
                .workspace_metadata(path, follow)
                .await
                .map_err(node_error);
        }
        #[cfg(test)]
        {
            let metadata = if follow {
                tokio::fs::metadata(path).await
            } else {
                tokio::fs::symlink_metadata(path).await
            };
            match metadata {
                Ok(meta) => Ok(aow_runtime_client::NodeMetadata {
                    exists: true,
                    file: meta.is_file(),
                    directory: meta.is_dir(),
                    symlink: meta.is_symlink(),
                    size: meta.len(),
                }),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(aow_runtime_client::NodeMetadata {
                        exists: false,
                        file: false,
                        directory: false,
                        symlink: false,
                        size: 0,
                    })
                }
                Err(error) => Err(error.into()),
            }
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Workspace runtime is unavailable".into()))
        }
    }
    pub(super) async fn node_mkdir(&self, path: &Path) -> Result<(), AowError> {
        if let Some(runtime) = self.node() {
            let _: serde_json::Value = runtime
                .workspace_operation("mkdir_all", serde_json::json!({"path":path}))
                .await
                .map_err(node_error)?;
            return Ok(());
        }
        #[cfg(test)]
        {
            Ok(tokio::fs::create_dir_all(path).await?)
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Workspace runtime is unavailable".into()))
        }
    }
    pub(super) async fn validate_absolute_directory(
        &self,
        value: &str,
    ) -> Result<PathBuf, AowError> {
        let path = Path::new(value);
        if !path.is_absolute() {
            return Err(AowError::Invalid(
                "Project path must be absolute on the node".into(),
            ));
        }
        self.canonical_directory(path).await
    }
    pub(super) async fn validate_new_worktree_path(
        &self,
        value: &str,
    ) -> Result<PathBuf, AowError> {
        let value = value.trim();
        let path = PathBuf::from(value);
        if value.is_empty() || value.len() > 4096 || !path.is_absolute() || value.contains('\0') {
            return Err(AowError::Invalid(
                "Worktree path must be absolute on the node".into(),
            ));
        }
        if self.node_metadata(&path, true).await?.exists {
            return Err(AowError::Invalid("Worktree path already exists".into()));
        }
        let parent = path
            .parent()
            .ok_or_else(|| AowError::Invalid("Worktree path needs a parent directory".into()))?;
        Ok(self.canonical_directory(parent).await?.join(
            path.file_name()
                .ok_or_else(|| AowError::Invalid("Worktree path needs a name".into()))?,
        ))
    }
    pub(super) async fn node_temporary_note(
        &self,
        root: &Path,
        extension: &str,
    ) -> Result<(PathBuf, String), AowError> {
        if let Some(runtime) = self.node() {
            let value: serde_json::Value = runtime
                .workspace_operation(
                    "temporary_note",
                    serde_json::json!({"root":root,"extension":extension}),
                )
                .await
                .map_err(node_error)?;
            return Ok((
                PathBuf::from(
                    value["path"]
                        .as_str()
                        .ok_or_else(|| AowError::Invalid("Invalid node note response".into()))?,
                ),
                value["name"]
                    .as_str()
                    .ok_or_else(|| AowError::Invalid("Invalid node note name".into()))?
                    .to_owned(),
            ));
        }
        #[cfg(test)]
        {
            notes::create_temporary_note_file(root, extension, aow_id::new_id).await
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Workspace runtime is unavailable".into()))
        }
    }
    pub(super) async fn node_account(&self) -> Result<String, AowError> {
        if let Some(runtime) = self.node() {
            let value: serde_json::Value = runtime
                .workspace_operation("account", serde_json::json!({}))
                .await
                .map_err(node_error)?;
            return value["name"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| AowError::Invalid("Node account is unavailable".into()));
        }
        #[cfg(test)]
        {
            notes::process_account_name()
        }
        #[cfg(not(test))]
        {
            Err(AowError::Invalid("Workspace runtime is unavailable".into()))
        }
    }
    pub(super) async fn prepare_notes_directory(&self, path: &Path) -> Result<String, AowError> {
        self.node_mkdir(path).await?;
        Ok(self
            .canonical_directory(path)
            .await?
            .to_string_lossy()
            .into_owned())
    }
}
