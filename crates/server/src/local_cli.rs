//! Same-user CLI access. This router is only bound to a private Unix socket;
//! it is deliberately not merged into the public HTTP router.

use crate::{AppState, HttpError, aow, terminal::agent_control};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    routing::{get, post},
};
use std::{
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub struct LocalCliServer {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl LocalCliServer {
    pub async fn shutdown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = (&mut self.task).await;
    }
}

impl Drop for LocalCliServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub async fn start_local_cli(state: AppState, state_dir: &Path) -> anyhow::Result<LocalCliServer> {
    let directory = std::fs::canonicalize(state_dir)?;
    let uid = unsafe { libc::geteuid() };
    anyhow::ensure!(
        std::fs::metadata(&directory)?.uid() == uid,
        "CLI socket directory must belong to the current user"
    );
    let directory = directory.join("cli");
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = std::fs::symlink_metadata(&directory)?;
    anyhow::ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == uid
            && metadata.permissions().mode() & 0o777 == 0o700,
        "CLI directory must be a same-user directory with mode 0700"
    );
    let path = directory.join("cli.sock");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_socket() && metadata.uid() == uid,
                "CLI socket path is not a same-user Unix socket"
            );
            match tokio::net::UnixStream::connect(&path).await {
                Ok(_) => {
                    anyhow::bail!("AoW CLI socket is already in use: {}", path.display())
                }
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    let current = std::fs::symlink_metadata(&path)?;
                    anyhow::ensure!(
                        current.dev() == metadata.dev() && current.ino() == metadata.ino(),
                        "CLI socket changed while checking it"
                    );
                    std::fs::remove_file(&path)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener = tokio::net::UnixListener::bind(&path)?;
    let metadata = std::fs::symlink_metadata(&path)?;
    let guard = SocketGuard {
        path: path.clone(),
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let router = Router::new()
        .route("/v1/projects", get(list_projects))
        .route("/v1/projects/{project_id}", get(get_project))
        .route(
            "/v1/agents",
            get(agent_control::list).post(agent_control::create),
        )
        .route("/v1/agents/{pane_id}", get(agent_control::get))
        .route("/v1/agents/{pane_id}/submit", post(agent_control::submit))
        .with_state(state);
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _guard = guard;
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = receiver.await;
            })
            .await
    });
    Ok(LocalCliServer {
        shutdown: Some(shutdown),
        task,
    })
}

async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let items = state
        .aow
        .registered_projects()
        .map_err(aow::aow_http_error)?;
    Ok(Json(serde_json::json!({"items": items})))
}

async fn get_project(
    State(state): State<AppState>,
    AxumPath(project_id): AxumPath<String>,
) -> Result<Json<aow::ProjectSummary>, HttpError> {
    state
        .aow
        .registered_project(&project_id)
        .map(Json)
        .map_err(aow::aow_http_error)
}

#[cfg(test)]
mod tests;
