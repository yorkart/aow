//! Project business workflows call node capabilities through the SDK.
use super::*;
use axum::http::StatusCode;
pub(crate) fn node_error(error: aow_runtime_client::RuntimeError) -> ServiceError {
    match error {
        aow_runtime_client::RuntimeError::HttpStatus {
            status, message, ..
        } if status == StatusCode::NOT_FOUND => {
            ServiceError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, message))
        }
        aow_runtime_client::RuntimeError::HttpStatus {
            status, message, ..
        } if status == StatusCode::FORBIDDEN => ServiceError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            message,
        )),
        error => ServiceError::Invalid(error.to_string()),
    }
}
impl RuntimeConnection {
    pub(crate) fn node(&self) -> Option<&aow_runtime_client::RuntimeClient> {
        self.inner.runtime_client.as_ref()
    }
    pub(crate) async fn git_output(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> Result<String, ServiceError> {
        let runtime = self.agent_node().await?;
        let output = runtime.workspace_git(cwd, args).await.map_err(node_error)?;
        if !output.success {
            return Err(ServiceError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        String::from_utf8(output.stdout).map_err(|e| ServiceError::Git(e.to_string()))
    }
    pub(crate) async fn optional_git_output(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> Result<Option<String>, ServiceError> {
        let runtime = self.agent_node().await?;
        let output = runtime.workspace_git(cwd, args).await.map_err(node_error)?;
        if output.success {
            return String::from_utf8(output.stdout)
                .map(Some)
                .map_err(|e| ServiceError::Git(e.to_string()));
        }
        if output.code == Some(1) && output.stderr.is_empty() {
            return Ok(None);
        }
        Err(ServiceError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
    pub(crate) async fn canonical_directory(&self, path: &Path) -> Result<PathBuf, ServiceError> {
        let runtime = self.agent_node().await?;
        let (path, directory) = runtime
            .workspace_canonicalize(path)
            .await
            .map_err(node_error)?;
        if !directory {
            return Err(ServiceError::Invalid("Node path is not a directory".into()));
        }
        Ok(path)
    }
    pub(crate) async fn node_metadata(
        &self,
        path: &Path,
        follow: bool,
    ) -> Result<aow_runtime_client::NodeMetadata, ServiceError> {
        let runtime = self.agent_node().await?;
        return runtime
            .workspace_metadata(path, follow)
            .await
            .map_err(node_error);
    }
    pub(crate) async fn node_mkdir(&self, path: &Path) -> Result<(), ServiceError> {
        let runtime = self.agent_node().await?;
        let () = runtime
            .workspace_mkdir_all(path)
            .await
            .map_err(node_error)?;
        Ok(())
    }
    pub(crate) async fn validate_absolute_directory(
        &self,
        value: &str,
    ) -> Result<PathBuf, ServiceError> {
        let path = Path::new(value);
        if !path.is_absolute() {
            return Err(ServiceError::Invalid(
                "Project path must be absolute on the node".into(),
            ));
        }
        self.canonical_directory(path).await
    }
    pub(crate) async fn validate_new_worktree_path(
        &self,
        value: &str,
    ) -> Result<PathBuf, ServiceError> {
        let value = value.trim();
        let path = PathBuf::from(value);
        if value.is_empty() || value.len() > 4096 || !path.is_absolute() || value.contains('\0') {
            return Err(ServiceError::Invalid(
                "Worktree path must be absolute on the node".into(),
            ));
        }
        if self.node_metadata(&path, true).await?.exists {
            return Err(ServiceError::Invalid("Worktree path already exists".into()));
        }
        let parent = path.parent().ok_or_else(|| {
            ServiceError::Invalid("Worktree path needs a parent directory".into())
        })?;
        Ok(self.canonical_directory(parent).await?.join(
            path.file_name()
                .ok_or_else(|| ServiceError::Invalid("Worktree path needs a name".into()))?,
        ))
    }
    pub(crate) async fn node_temporary_note(
        &self,
        root: &Path,
        extension: &str,
    ) -> Result<(PathBuf, String), ServiceError> {
        let runtime = self.agent_node().await?;
        let note = runtime
            .workspace_temporary_note(root, extension)
            .await
            .map_err(node_error)?;
        Ok((note.path, note.name))
    }
    pub(crate) async fn node_account(&self) -> Result<String, ServiceError> {
        let runtime = self.agent_node().await?;
        return runtime.workspace_account().await.map_err(node_error);
    }
    pub(crate) async fn prepare_notes_directory(
        &self,
        path: &Path,
    ) -> Result<String, ServiceError> {
        self.node_mkdir(path).await?;
        Ok(self
            .canonical_directory(path)
            .await?
            .to_string_lossy()
            .into_owned())
    }
}
