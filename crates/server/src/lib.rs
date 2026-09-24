use std::{
    ffi::CStr,
    io::SeekFrom,
    path::{Component, Path, PathBuf},
    sync::LazyLock,
};

use aow_filesystem::{
    DEFAULT_MAX_TEXT_BYTES, FsError, list_directory, open_file, read_text_file, rename_entry,
    rename_file, write_atomic_stream, write_unique_stream,
};
use aow_git_service as git;
use aow_protocol::{ApiError, RenameResult};
use aow_terminald_client::TerminaldClient;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
            CONTENT_TYPE, ETAG, IF_MATCH, LOCATION, RANGE,
        },
    },
    response::{IntoResponse, Response},
    routing::{get, put},
};
use futures_util::TryStreamExt;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

mod auth;
mod base_path;
pub use base_path::BasePath;
mod local_cli;
pub use local_cli::start_local_cli;
mod aow;
mod automations;
mod im;
mod notifications;
mod operations;
mod pull_requests;
mod session_shares;
mod terminal;
pub use terminal::{TerminalError, default_state_dir as default_terminal_state_dir};

static PROCESS_HOME: LazyLock<PathBuf> = LazyLock::new(process_home);
const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

#[derive(Clone)]
pub struct AppState {
    base_path: BasePath,
    frontend_dist: PathBuf,
    auth: auth::PinAuth,
    session_shares: session_shares::SessionShares,
    terminals: terminal::TerminalManager,
    aow: aow::AowManager,
    automations: Option<automations::AutomationManager>,
    operations: operations::OperationService,
    review_providers: pull_requests::ProviderManager,
}

impl AppState {
    /// Creates an application state with in-memory terminal metadata. This is
    /// useful for embedding and tests; the server binary uses `with_state_dir`.
    pub fn new(frontend_dist: PathBuf) -> Self {
        Self::with_terminald_socket(frontend_dist, TerminaldClient::default_socket_path())
    }

    pub fn with_terminald_socket(frontend_dist: PathBuf, terminald_socket: PathBuf) -> Self {
        Self {
            base_path: BasePath::default(),
            frontend_dist,
            auth: auth::PinAuth::disabled(),
            session_shares: session_shares::SessionShares::in_memory(),
            terminals: terminal::TerminalManager::in_memory(TerminaldClient::new(terminald_socket)),
            aow: aow::AowManager::in_memory(),
            automations: None,
            review_providers: pull_requests::ProviderManager::default(),
            operations: operations::OperationService::in_memory(),
        }
    }

    pub fn with_state_dir(
        frontend_dist: PathBuf,
        state_dir: PathBuf,
        terminald_socket: PathBuf,
    ) -> Result<Self, TerminalError> {
        Self::with_runtime_options(frontend_dist, state_dir, terminald_socket)
    }

    pub fn with_runtime_options(
        frontend_dist: PathBuf,
        state_dir: PathBuf,
        terminald_socket: PathBuf,
    ) -> Result<Self, TerminalError> {
        let aow = aow::AowManager::persistent(&state_dir).map_err(|error| {
            TerminalError::Invalid(format!("failed to initialize aow: {error}"))
        })?;
        Ok(Self {
            review_providers: pull_requests::ProviderManager::persistent(&state_dir)
                .map_err(|e| TerminalError::Invalid(e.to_string()))?,
            base_path: BasePath::default(),
            frontend_dist,
            operations: operations::OperationService::persistent(&state_dir.join("operation-logs"))
                .map_err(|error| TerminalError::Invalid(error.to_string()))?,
            auth: auth::PinAuth::persistent(&state_dir),
            session_shares: session_shares::SessionShares::persistent(&state_dir).map_err(
                |error| {
                    TerminalError::Invalid(format!("failed to initialize session shares: {error}"))
                },
            )?,
            automations: Some(
                automations::AutomationManager::new(state_dir.clone(), aow.notifications().clone())
                    .map_err(|error| {
                        TerminalError::Invalid(format!("failed to initialize automations: {error}"))
                    })?,
            ),
            terminals: terminal::TerminalManager::persistent(
                state_dir.clone(),
                TerminaldClient::new(terminald_socket),
            )?,
            aow,
        })
    }

    pub fn with_base_path(mut self, base_path: BasePath) -> Self {
        self.auth.set_cookie_name(base_path.cookie_name());
        self.base_path = base_path;
        self
    }

    /// Reconciles durable terminal tabs with the external terminal daemon.
    /// A later terminal request retries this operation if startup happens
    /// while the daemon is unavailable.
    pub async fn reconcile_terminals(&self) -> Result<(), TerminalError> {
        self.terminals.reconcile().await
    }

    pub fn start_agent_notifications(&self) {
        self.terminals.start_agent_notifications(self.aow.clone());
    }

    pub async fn initialize_global_workspace(&self) -> anyhow::Result<()> {
        self.aow.initialize_global().await?;
        Ok(())
    }
}

pub async fn initialize_aow_state(path: &std::path::Path) -> anyhow::Result<()> {
    pull_requests::ProviderManager::persistent(path)?;
    aow::AowManager::persistent(path)?
        .initialize_global()
        .await?;
    Ok(())
}

pub fn build_router(state: AppState) -> Router {
    let base_path = state.base_path.clone();
    let app = Router::new()
        .route("/api/auth/status", get(auth::status))
        .route("/api/auth/login", axum::routing::post(auth::login))
        .route("/api/health", get(health))
        .route("/api/fs/home", get(list_home))
        .route("/api/fs/tree", get(list_root))
        .route("/api/fs/tree/{*path}", get(list_path))
        .route("/api/fs/text/{*path}", get(read_text))
        .route("/api/fs/raw/{*path}", get(raw_file))
        .route(
            "/api/fs/file/{*path}",
            put(write_file).patch(rename_file_path),
        )
        .route(
            "/api/fs/entries",
            axum::routing::post(create_fs_entry)
                .patch(rename_fs_entry)
                .delete(delete_fs_entry),
        )
        .route("/api/git/repositories", get(git_repositories))
        .route("/api/git/ignored", axum::routing::post(git_ignored))
        .route("/api/git/status", get(git_status))
        .route("/api/git/pull", axum::routing::post(git_pull))
        .route("/api/git/push", axum::routing::post(git_push))
        .route("/api/git/log", get(git_log))
        .route("/api/git/diff", get(git_diff))
        .route("/api/git/commit/detail", get(git_commit_detail))
        .route("/api/git/commit/files", get(git_commit_files))
        .route("/api/git/commit/diff", get(git_commit_diff))
        .route("/api/my-pull-requests", get(my_pull_requests))
        .route(
            "/api/my-pull-requests/{number}",
            get(my_pull_request_detail),
        )
        .route(
            "/api/my-pull-requests/{number}/diff",
            get(my_pull_request_diff),
        )
        .route("/", get(root_redirect))
        .route("/aow", get(aow_root_redirect))
        .route("/aow/", get(aow_root))
        .route("/aow/tabs/{tab_id}", get(aow_root))
        .route("/aow/tabs/{tab_id}/", get(aow_root))
        .route("/aow/tabs/terminal/{tab_id}", get(aow_root))
        .route("/aow/tabs/pr/{provider}/{number}", get(aow_root))
        .route("/aow/tabs/session/{agent}/{session_id}", get(aow_root))
        .route("/aow/tabs/automation/{task_id}", get(aow_root))
        .route(
            "/aow/tabs/automation/{task_id}/runs/{run_id}",
            get(aow_root),
        )
        .route("/m", get(aow_root))
        .route("/m/", get(aow_root))
        .route("/fs", get(fs_root_redirect))
        .route("/fs/", get(fs_root))
        .route("/fs/{*path}", get(fs_path))
        .route("/help", get(help_page))
        .merge(terminal::routes())
        .merge(aow::routes())
        .merge(pull_requests::routes())
        .merge(notifications::routes())
        .merge(operations::routes())
        .merge(automations::routes())
        .merge(session_shares::routes())
        .fallback(spa_or_asset)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    Router::new()
        .fallback_service(app)
        .layer(axum::middleware::from_fn_with_state(
            base_path,
            base_path::mount,
        ))
}

async fn health() -> Json<serde_json::Value> {
    let health =
        serde_json::json!({ "ok": true, "service": "aow", "version": env!("CARGO_PKG_VERSION") });
    #[cfg(target_os = "macos")]
    let health = {
        let mut health = health;
        health["pid"] = std::process::id().into();
        health
    };
    Json(health)
}

async fn root_redirect(
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let location = query
        .get("ui")
        .filter(|mode| matches!(mode.as_str(), "desktop" | "mobile"))
        .map_or_else(|| "/aow/".to_owned(), |mode| format!("/aow/?ui={mode}"));
    (StatusCode::PERMANENT_REDIRECT, [(LOCATION, location)])
}

async fn aow_root_redirect(
    query: Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    root_redirect(query).await
}

async fn aow_root(State(state): State<AppState>) -> Result<Response, HttpError> {
    serve_spa_index(&state).await
}

async fn list_root() -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(list_directory("/").await?))
}

async fn list_home() -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(list_directory(&*PROCESS_HOME).await?))
}

async fn list_path(
    AxumPath(path): AxumPath<String>,
) -> Result<Json<aow_protocol::DirectoryListing>, HttpError> {
    Ok(Json(list_directory(decode_absolute(&path)?).await?))
}

async fn read_text(AxumPath(path): AxumPath<String>) -> Result<impl IntoResponse, HttpError> {
    let text = read_text_file(decode_absolute(&path)?, DEFAULT_MAX_TEXT_BYTES).await?;
    let etag = HeaderValue::from_str(&format!("\"{}\"", text.version))
        .map_err(|error| HttpError::internal(error.to_string()))?;
    let mut headers = HeaderMap::new();
    headers.insert(ETAG, etag);
    Ok((headers, Json(text)))
}

#[derive(Default, Deserialize)]
struct WriteFileQuery {
    #[serde(default)]
    keep_both: bool,
}

async fn write_file(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    Query(query): Query<WriteFileQuery>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let path = decode_absolute(&path)?;
    let expected = headers
        .get(IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('\"'));
    let stream = body.into_data_stream().map_err(|error| error.to_string());
    let result = if query.keep_both {
        write_unique_stream(path, stream).await?
    } else {
        write_atomic_stream(path, expected, stream).await?
    };
    let status = if result.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        ETAG,
        HeaderValue::from_str(&format!("\"{}\"", result.version))
            .map_err(|error| HttpError::internal(error.to_string()))?,
    );
    Ok((status, response_headers, Json(result)))
}

#[derive(Deserialize)]
struct RenameFileRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CreateFsEntryRequest {
    parent: String,
    name: String,
    kind: FsEntryKind,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FsEntryKind {
    File,
    Directory,
}

#[derive(Debug, Deserialize)]
struct RenameFsEntryRequest {
    path: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct FsEntryPathQuery {
    path: String,
}

async fn rename_file_path(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    Json(request): Json<RenameFileRequest>,
) -> Result<Json<RenameResult>, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let destination = rename_file(decode_absolute(&path)?, &request.name).await?;
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| HttpError::internal("renamed file does not have a UTF-8 name"))?
        .to_owned();
    Ok(Json(RenameResult {
        path: destination.to_string_lossy().into_owned(),
        name,
    }))
}

async fn create_fs_entry(
    State(state): State<AppState>,
    Json(request): Json<CreateFsEntryRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let name = request.name.trim().to_owned();
    validate_fs_entry_name(&name)?;
    let parent = PathBuf::from(&request.parent);
    validate_fs_entry_path(&parent)?;
    if !tokio::fs::metadata(&parent).await?.is_dir() {
        return Err(FsError::NotDirectory(parent.to_string_lossy().into_owned()).into());
    }
    let path = parent.join(&name);
    let result = match request.kind {
        FsEntryKind::File => tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
            .map(|_| ("file", StatusCode::CREATED)),
        FsEntryKind::Directory => tokio::fs::create_dir(&path)
            .await
            .map(|_| ("directory", StatusCode::CREATED)),
    };
    let (kind, status) = result.map_err(fs_entry_io_error)?;
    Ok((
        status,
        Json(serde_json::json!({
            "path": path.to_string_lossy(),
            "name": name,
            "kind": kind,
        })),
    ))
}

async fn rename_fs_entry(
    State(state): State<AppState>,
    Json(request): Json<RenameFsEntryRequest>,
) -> Result<Json<RenameResult>, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let path = PathBuf::from(&request.path);
    validate_fs_entry_path(&path)?;
    let destination = rename_entry(path, &request.name).await?;
    Ok(Json(RenameResult {
        path: destination.to_string_lossy().into_owned(),
        name: request.name.trim().to_owned(),
    }))
}

async fn delete_fs_entry(
    State(state): State<AppState>,
    Query(query): Query<FsEntryPathQuery>,
) -> Result<StatusCode, HttpError> {
    let _filesystem = state.aow.filesystem_access().await;
    let path = PathBuf::from(&query.path);
    validate_fs_entry_path(&path)?;
    if path.parent().is_none() {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "root_delete_forbidden",
            "filesystem root cannot be deleted",
            Some(path.to_string_lossy().into_owned()),
        ));
    }
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        tokio::fs::remove_file(&path).await?;
    } else if metadata.is_dir() {
        tokio::fs::remove_dir_all(&path).await?;
    } else {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_entry",
            "only files, directories, and symbolic links can be deleted",
            Some(path.to_string_lossy().into_owned()),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn validate_fs_entry_name(name: &str) -> Result<(), HttpError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.len() > 255
        || trimmed.contains(['/', '\\', '\0'])
        || Path::new(trimmed)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(trimmed)
    {
        return Err(FsError::InvalidFileName(name.to_owned()).into());
    }
    Ok(())
}

fn validate_fs_entry_path(path: &Path) -> Result<(), HttpError> {
    if !path.is_absolute() {
        return Err(FsError::PathNotAbsolute(path.to_string_lossy().into_owned()).into());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "invalid_path",
            "path must not contain . or .. components",
            Some(path.to_string_lossy().into_owned()),
        ));
    }
    Ok(())
}

fn fs_entry_io_error(error: std::io::Error) -> HttpError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        HttpError::new(
            StatusCode::CONFLICT,
            "destination_exists",
            error.to_string(),
            None,
        )
    } else {
        error.into()
    }
}

async fn raw_file(
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let path = decode_absolute(&path)?;
    raw_response(path, headers, false).await
}

async fn raw_response(
    path: PathBuf,
    headers: HeaderMap,
    attachment: bool,
) -> Result<Response, HttpError> {
    let (mut file, metadata) = open_file(&path).await?;
    let size = metadata.len();
    let content_type = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    let range = headers.get(RANGE).and_then(|value| value.to_str().ok());
    let (start, end, status) = match range {
        Some(value) => {
            let (start, end) = parse_range(value, size)?;
            (start, end, StatusCode::PARTIAL_CONTENT)
        }
        None => (0, size.saturating_sub(1), StatusCode::OK),
    };
    let length = if size == 0 { 0 } else { end - start + 1 };
    file.seek(SeekFrom::Start(start)).await?;
    let stream = ReaderStream::new(file.take(length));
    let body = Body::from_stream(stream);
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let response_headers = response.headers_mut();
    response_headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_str(&content_type).unwrap());
    if attachment {
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        response_headers.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!(
                "attachment; filename*=UTF-8''{}",
                utf8_percent_encode(&filename, PATH_SEGMENT_ENCODE_SET)
            ))
            .unwrap(),
        );
    }
    response_headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).unwrap(),
    );
    if status == StatusCode::PARTIAL_CONTENT {
        response_headers.insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}")).unwrap(),
        );
    }
    Ok(response)
}

#[derive(Deserialize)]
struct RepositoryQuery {
    root: String,
    #[serde(default = "default_depth")]
    depth: usize,
}

fn default_depth() -> usize {
    4
}

async fn git_repositories(
    Query(query): Query<RepositoryQuery>,
) -> Result<Json<Vec<aow_protocol::RepositorySummary>>, HttpError> {
    Ok(Json(
        git::discover_repositories(query.root, query.depth.min(8)).await?,
    ))
}

#[derive(Deserialize)]
struct IgnoredPathsRequest {
    root: String,
    paths: Vec<PathBuf>,
}

async fn git_ignored(
    Json(request): Json<IgnoredPathsRequest>,
) -> Result<Json<aow_protocol::GitIgnoredPaths>, HttpError> {
    Ok(Json(git::ignored_paths(request.root, request.paths).await?))
}

#[derive(Deserialize)]
struct RepoOnlyQuery {
    repo: String,
}

async fn git_status(
    Query(query): Query<RepoOnlyQuery>,
) -> Result<Json<aow_protocol::GitStatus>, HttpError> {
    Ok(Json(git::status(query.repo).await?))
}

async fn git_pull(Json(request): Json<RepoOnlyQuery>) -> Result<StatusCode, HttpError> {
    git::pull(request.repo).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn git_push(Json(request): Json<RepoOnlyQuery>) -> Result<StatusCode, HttpError> {
    git::push(request.repo).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct LogQuery {
    repo: String,
    #[serde(default = "default_log_limit")]
    limit: usize,
}

fn default_log_limit() -> usize {
    100
}

async fn git_log(Query(query): Query<LogQuery>) -> Result<Json<aow_protocol::GitLog>, HttpError> {
    Ok(Json(git::log(query.repo, query.limit).await?))
}

#[derive(Deserialize)]
struct DiffQuery {
    repo: String,
    path: Option<String>,
    #[serde(default)]
    staged: bool,
}

async fn git_diff(
    Query(query): Query<DiffQuery>,
) -> Result<Json<aow_protocol::GitDiff>, HttpError> {
    Ok(Json(
        git::diff(query.repo, query.path.as_deref(), query.staged).await?,
    ))
}

#[derive(Deserialize)]
struct CommitQuery {
    repo: String,
    commit: String,
}

async fn git_commit_detail(
    State(state): State<AppState>,
    Query(query): Query<CommitQuery>,
) -> Result<Json<aow_protocol::GitCommitDetail>, HttpError> {
    let mut detail = git::commit_detail(&query.repo, &query.commit).await?;
    if let Some(remote) = &detail.remote_name {
        // Links are optional enrichment. A slow or broken Provider must not
        // prevent local commit metadata from being displayed.
        let links = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let paths = state
                .aow
                .execution_path()
                .await
                .map_err(|e| pull_requests::PullRequestError::Command(e.to_string()))?;
            state
                .review_providers
                .commit_links(&query.repo, remote, &detail.id, &paths)
                .await
        })
        .await;
        match links {
            Ok(Ok(Some(links))) => {
                detail.remote_url = links.remote_url;
                detail.commit_url = links.commit_url;
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "commit links Provider failed"),
            Err(_) => tracing::warn!("commit links Provider timed out"),
        }
    }
    Ok(Json(detail))
}

async fn git_commit_files(
    Query(query): Query<CommitQuery>,
) -> Result<Json<aow_protocol::GitCommitFiles>, HttpError> {
    Ok(Json(git::commit_files(query.repo, &query.commit).await?))
}

#[derive(Deserialize)]
struct CommitDiffQuery {
    repo: String,
    commit: String,
    path: String,
    original_path: Option<String>,
}

async fn git_commit_diff(
    Query(query): Query<CommitDiffQuery>,
) -> Result<Json<aow_protocol::GitDiff>, HttpError> {
    Ok(Json(
        git::commit_diff(
            query.repo,
            &query.commit,
            &query.path,
            query.original_path.as_deref(),
        )
        .await?,
    ))
}

async fn my_pull_requests(
    State(state): State<AppState>,
    Query(query): Query<pull_requests::ReviewQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    Ok(Json(
        state
            .review_providers
            .call(
                &query,
                "list",
                serde_json::json!({}),
                &state
                    .aow
                    .execution_path()
                    .await
                    .map_err(|e| pull_requests::PullRequestError::Command(e.to_string()))?,
            )
            .await?,
    ))
}

async fn my_pull_request_detail(
    State(state): State<AppState>,
    AxumPath(number): AxumPath<u64>,
    Query(query): Query<pull_requests::ReviewQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    pull_requests::positive(number)?;
    Ok(Json(
        state
            .review_providers
            .call(
                &query,
                "detail",
                serde_json::json!({"number":number}),
                &state
                    .aow
                    .execution_path()
                    .await
                    .map_err(|e| pull_requests::PullRequestError::Command(e.to_string()))?,
            )
            .await?,
    ))
}

async fn my_pull_request_diff(
    State(state): State<AppState>,
    AxumPath(number): AxumPath<u64>,
    Query(query): Query<pull_requests::DiffQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    pull_requests::positive(number)?;
    pull_requests::safe_path(&query.path)?;
    Ok(Json(state.review_providers.call(&query.target, "diff", serde_json::json!({"number":number,"path":query.path,"patch_only":query.patch_only}), &state.aow.execution_path().await.map_err(|e| pull_requests::PullRequestError::Command(e.to_string()))?).await?))
}

async fn fs_root_redirect() -> impl IntoResponse {
    (StatusCode::PERMANENT_REDIRECT, [(LOCATION, "/fs/")])
}

async fn fs_root(State(state): State<AppState>) -> Result<Response, HttpError> {
    fs_browser(PathBuf::from("/"), &state.base_path).await
}

async fn fs_path(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let path = decode_absolute(&path)?;
    if tokio::fs::metadata(&path).await?.is_file() {
        return raw_response(path, headers, true).await;
    }
    fs_browser(path, &state.base_path).await
}

async fn fs_browser(path: PathBuf, base_path: &BasePath) -> Result<Response, HttpError> {
    let base = base_path.as_str();
    let listing = list_directory(&path).await?;
    let home = PROCESS_HOME.to_string_lossy();
    let rows = listing
        .entries
        .iter()
        .map(|entry| {
            let encoded = encode_absolute(&entry.path);
            let kind = match (entry.is_symlink, &entry.kind) {
                (true, _) => "符号链接",
                (_, aow_protocol::FileKind::Directory) => "目录",
                (_, aow_protocol::FileKind::File) => "文件",
                (_, aow_protocol::FileKind::Symlink) => "符号链接",
                (_, aow_protocol::FileKind::Other) => "其他",
            };
            let directory = matches!(
                entry.kind,
                aow_protocol::FileKind::Directory
            );
            let slash = if directory { "/" } else { "" };
            let icon = if directory { "📁" } else { "📄" };
            let link = entry
                .link_target
                .as_deref()
                .map(|target| format!(" → {}", escape_html(target)))
                .unwrap_or_default();
            format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{kind}</td><td><a href=\"{base}/fs{encoded}{slash}\">{icon} {}{slash}</a>{link}</td><td><button class=\"copy\" data-path=\"{}\" title=\"复制服务器路径\">⧉</button></td></tr>",
                mode_string(entry.mode),
                entry.links,
                entry.uid,
                entry.gid,
                entry.size,
                entry
                    .modified_ms
                    .and_then(|value| chrono::DateTime::from_timestamp_millis(value as i64))
                    .map(|value| value.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| "-".to_owned()),
                escape_html(&entry.name),
                escape_html(&entry.path)
            )
        })
        .collect::<String>();
    let parent = path.parent().unwrap_or(Path::new("/"));
    let current = escape_html(&path.to_string_lossy());
    let encoded_current = encode_absolute(&path.to_string_lossy());
    let upload_base = format!(
        "{base}/api/fs/file{}",
        encoded_current.trim_end_matches('/')
    );
    let html = format!(
        r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>文件浏览器 - {current}</title><style>
body{{margin:0;background:#1e1e1e;color:#ccc;font:12px system-ui}}a{{color:#4daafc;text-decoration:none}}header{{display:flex;align-items:center;gap:5px;padding:7px;border-bottom:1px solid #444}}header code{{flex:1;overflow:hidden;text-overflow:ellipsis}}button,.button{{padding:4px 8px;border:1px solid #555;border-radius:4px;background:#2d2d2d;color:#ddd;cursor:pointer}}main{{padding:6px}}table{{width:100%;border-collapse:collapse}}th,td{{padding:4px 7px;border-bottom:1px solid #393939;text-align:left}}tr:hover{{background:#292929}}.copy{{padding:2px 7px}}#status{{color:#9cdcfe}}
</style></head><body><header><a class="button" href="{base}/fs{parent}/">..</a><a class="button" href="{base}/fs{home}/">Home</a><a class="button" href="{base}/fs/tmp/">/tmp</a><a class="button" href="{base}/aow/">Project AOW</a><code>{current}</code><input id="files" type="file" multiple><button id="upload">上传</button><span id="status"></span></header><main><table><thead><tr><th>权限</th><th>链接</th><th>UID</th><th>GID</th><th>大小</th><th>修改时间</th><th>类型</th><th>名称</th><th>操作</th></tr></thead><tbody>{rows}</tbody></table></main><script>
document.addEventListener('click',async e=>{{const b=e.target.closest('.copy');if(b){{await navigator.clipboard.writeText(b.dataset.path);b.textContent='✓';setTimeout(()=>b.textContent='⧉',1000)}}}});
document.getElementById('upload').onclick=async()=>{{const files=[...document.getElementById('files').files];for(const file of files){{document.getElementById('status').textContent='上传 '+file.name;const response=await fetch('{upload_base}/'+encodeURIComponent(file.name),{{method:'PUT',body:file}});if(!response.ok)throw new Error(await response.text())}}location.reload()}};
</script></body></html>"#,
        parent = encode_absolute(&parent.to_string_lossy()).trim_end_matches('/'),
        home = encode_absolute(&home).trim_end_matches('/')
    );
    Ok(axum::response::Html(html).into_response())
}

async fn help_page(State(state): State<AppState>) -> axum::response::Html<String> {
    let mut schema = serde_json::json!({
        "service": "aow",
        "tools": [
            {"name":"list_directory","method":"GET","path":"/api/fs/tree{absolute_path}"},
            {"name":"read_text_file","method":"GET","path":"/api/fs/text{absolute_path}"},
            {"name":"write_file","method":"PUT","path":"/api/fs/file{absolute_path}"},
            {"name":"rename_file","method":"PATCH","path":"/api/fs/file{absolute_path}","body":{"name":"new-name.ext"}},
            {"name":"create_entry","method":"POST","path":"/api/fs/entries","body":{"parent":"/absolute/path","name":"new-entry","kind":"file|directory"}},
            {"name":"rename_entry","method":"PATCH","path":"/api/fs/entries","body":{"path":"/absolute/path","name":"new-name"}},
            {"name":"delete_entry","method":"DELETE","path":"/api/fs/entries?path={absolute_path}"},
            {"name":"discover_repositories","method":"GET","path":"/api/git/repositories?root={absolute_path}"},
            {"name":"git_ignored","method":"POST","path":"/api/git/ignored","body":{"root":"/absolute/explorer/root","paths":["/absolute/explorer/root/entry"]}},
            {"name":"git_status","method":"GET","path":"/api/git/status?repo={absolute_path}"},
            {"name":"git_pull","method":"POST","path":"/api/git/pull","body":{"repo":"/absolute/repository/path"}},
            {"name":"git_push","method":"POST","path":"/api/git/push","body":{"repo":"/absolute/repository/path"}},
            {"name":"git_log","method":"GET","path":"/api/git/log?repo={absolute_path}"},
            {"name":"git_diff","method":"GET","path":"/api/git/diff?repo={absolute_path}&path={relative_path}"}
            ,{"name":"git_commit_detail","method":"GET","path":"/api/git/commit/detail?repo={absolute_path}&commit={commit_id}"}
            ,{"name":"git_commit_files","method":"GET","path":"/api/git/commit/files?repo={absolute_path}&commit={commit_id}"}
            ,{"name":"git_commit_diff","method":"GET","path":"/api/git/commit/diff?repo={absolute_path}&commit={commit_id}&path={relative_path}"}
            ,{"name":"my_pull_requests","method":"GET","path":"/api/my-pull-requests?repo={absolute_path}","description":"Open reviews from the configured provider"}
            ,{"name":"my_pull_request_detail","method":"GET","path":"/api/my-pull-requests/{number}?repo={absolute_path}","description":"Review detail from the configured provider"}
            ,{"name":"my_pull_request_diff","method":"GET","path":"/api/my-pull-requests/{number}/diff?repo={absolute_path}&path={relative_path}","description":"Review file diff from the configured provider"}
        ]
    });
    for tool in schema["tools"].as_array_mut().unwrap() {
        tool["path"] = state.base_path.url(tool["path"].as_str().unwrap()).into();
    }
    axum::response::Html(format!(
        "<!doctype html><meta charset=utf-8><title>AOW API</title><h1>AOW API</h1><pre>{}</pre>",
        escape_html(&serde_json::to_string_pretty(&schema).unwrap())
    ))
}

async fn spa_or_asset(
    State(state): State<AppState>,
    uri: axum::http::Uri,
) -> Result<Response, HttpError> {
    let requested = uri.path().trim_start_matches('/');
    if requested == "index.html" {
        return serve_spa_index(&state).await;
    }
    let safe_asset = !requested.is_empty()
        && Path::new(requested)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    let candidate = state.frontend_dist.join(requested);
    if !safe_asset || !candidate.is_file() {
        return Err(HttpError::new(
            StatusCode::NOT_FOUND,
            "route_not_found",
            "route not found",
            Some(uri.path().to_owned()),
        ));
    }
    let path = candidate;
    let bytes = tokio::fs::read(&path).await.map_err(|error| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "frontend_not_built",
            format!("frontend asset unavailable at {}: {error}", path.display()),
            None,
        )
    })?;
    let content_type = mime_guess::from_path(&path).first_or_octet_stream();
    let cache_control = if requested.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Ok((
        [
            (CONTENT_TYPE, content_type.as_ref()),
            (CACHE_CONTROL, cache_control),
        ],
        bytes,
    )
        .into_response())
}

async fn serve_spa_index(state: &AppState) -> Result<Response, HttpError> {
    let path = state.frontend_dist.join("index.html");
    let html = tokio::fs::read_to_string(&path).await.map_err(|error| {
        HttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "frontend_not_built",
            format!("frontend asset unavailable at {}: {error}", path.display()),
            None,
        )
    })?;
    Ok((
        [
            (CONTENT_TYPE, "text/html; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        state.base_path.inject_html(&html),
    )
        .into_response())
}

fn decode_absolute(path: &str) -> Result<PathBuf, HttpError> {
    // Axum's Path extractor has already percent-decoded and UTF-8 validated the value.
    // Decoding it again would turn a literal filename such as `%20.txt` into a space.
    let path = Path::new("/").join(path.trim_start_matches('/'));
    Ok(path)
}

#[cfg(unix)]
fn process_home() -> PathBuf {
    // SAFETY: Called once through LazyLock; pw_dir is copied before returning.
    unsafe {
        let account = libc::getpwuid(libc::geteuid());
        if !account.is_null() && !(*account).pw_dir.is_null() {
            return PathBuf::from(
                CStr::from_ptr((*account).pw_dir)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    PathBuf::from("/")
}

#[cfg(not(unix))]
fn process_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn encode_absolute(path: &str) -> String {
    path.split('/')
        .map(|component| utf8_percent_encode(component, PATH_SEGMENT_ENCODE_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}

fn mode_string(mode: u32) -> String {
    let kind = match mode & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o100000 => '-',
        0o060000 => 'b',
        0o020000 => 'c',
        0o010000 => 'p',
        0o140000 => 's',
        _ => '?',
    };
    let mut value = String::with_capacity(10);
    value.push(kind);
    for (mask, character) in [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ] {
        value.push(if mode & mask != 0 { character } else { '-' });
    }
    value
}

fn parse_range(value: &str, size: u64) -> Result<(u64, u64), HttpError> {
    if size == 0 || !value.starts_with("bytes=") || value.contains(',') {
        return Err(HttpError::range_not_satisfiable(size));
    }
    let (start, end) = value[6..]
        .split_once('-')
        .ok_or_else(|| HttpError::range_not_satisfiable(size))?;
    let (start, end) = if start.is_empty() {
        let suffix = end
            .parse::<u64>()
            .map_err(|_| HttpError::range_not_satisfiable(size))?;
        let suffix = suffix.min(size);
        (size - suffix, size - 1)
    } else {
        let start = start
            .parse::<u64>()
            .map_err(|_| HttpError::range_not_satisfiable(size))?;
        let end = if end.is_empty() {
            size - 1
        } else {
            end.parse::<u64>()
                .map_err(|_| HttpError::range_not_satisfiable(size))?
                .min(size - 1)
        };
        (start, end)
    };
    if start >= size || start > end {
        return Err(HttpError::range_not_satisfiable(size));
    }
    Ok((start, end))
}

#[derive(Debug)]
pub struct HttpError {
    status: StatusCode,
    body: ApiError,
    extra_headers: Box<HeaderMap>,
}

impl HttpError {
    fn new(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
        path: Option<String>,
    ) -> Self {
        Self {
            status,
            body: ApiError {
                code: code.into(),
                message: message.into(),
                path,
            },
            extra_headers: Box::new(HeaderMap::new()),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            message,
            None,
        )
    }

    fn range_not_satisfiable(size: u64) -> Self {
        let mut error = Self::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "range_not_satisfiable",
            "requested byte range is not satisfiable",
            None,
        );
        error.extra_headers.insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{size}")).unwrap(),
        );
        error
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body)).into_response();
        response.headers_mut().extend(*self.extra_headers);
        response
    }
}

impl From<std::io::Error> for HttpError {
    fn from(error: std::io::Error) -> Self {
        let status = match error.kind() {
            std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
            std::io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, "io_error", error.to_string(), None)
    }
}

impl From<FsError> for HttpError {
    fn from(error: FsError) -> Self {
        match error {
            FsError::PathNotAbsolute(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "path_not_absolute",
                "path must be absolute",
                Some(path),
            ),
            FsError::NotDirectory(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "not_directory",
                "path is not a directory",
                Some(path),
            ),
            FsError::NotFile(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "not_file",
                "path is not a regular file",
                Some(path),
            ),
            FsError::TooLarge {
                path,
                size,
                maximum,
            } => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "file_too_large",
                format!("file is {size} bytes; maximum is {maximum}"),
                Some(path),
            ),
            FsError::NotUtf8(path) => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "not_utf8",
                "file is not UTF-8",
                Some(path),
            ),
            FsError::Binary(path) => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "binary_file",
                "file contains NUL bytes",
                Some(path),
            ),
            FsError::VersionConflict {
                path,
                expected,
                current,
            } => Self::new(
                StatusCode::PRECONDITION_FAILED,
                "version_conflict",
                format!("expected {expected}, current {current}"),
                Some(path),
            ),
            FsError::InvalidFileName(name) => Self::new(
                StatusCode::BAD_REQUEST,
                "invalid_file_name",
                "name must be a single non-empty file name",
                Some(name),
            ),
            FsError::AlreadyExists(path) => Self::new(
                StatusCode::CONFLICT,
                "destination_exists",
                "a file or directory with that name already exists",
                Some(path),
            ),
            FsError::Stream(message) => Self::new(
                StatusCode::BAD_REQUEST,
                "request_stream_failed",
                message,
                None,
            ),
            FsError::Io(error) => error.into(),
        }
    }
}

impl From<git::GitError> for HttpError {
    fn from(error: git::GitError) -> Self {
        match error {
            git::GitError::PathNotAbsolute(path) => Self::new(
                StatusCode::BAD_REQUEST,
                "path_not_absolute",
                "repository path must be absolute",
                Some(path),
            ),
            git::GitError::NotRepository(path) => Self::new(
                StatusCode::NOT_FOUND,
                "git_repository_not_found",
                "the terminal's current directory is not inside a Git repository",
                Some(path),
            ),
            git::GitError::Timeout => Self::new(
                StatusCode::GATEWAY_TIMEOUT,
                "git_timeout",
                error.to_string(),
                None,
            ),
            git::GitError::Command(_) => Self::new(
                StatusCode::BAD_REQUEST,
                "git_command_failed",
                error.to_string(),
                None,
            ),
            git::GitError::InvalidUtf8 => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "git_invalid_utf8",
                error.to_string(),
                None,
            ),
            git::GitError::Io(error) => error.into(),
            git::GitError::WalkDir(error) => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "repository_discovery_failed",
                error.to_string(),
                None,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use tempfile::TempDir;
    use tower::ServiceExt;

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(parse_range("bytes=2-5", 10).unwrap(), (2, 5));
        assert_eq!(parse_range("bytes=7-", 10).unwrap(), (7, 9));
        assert_eq!(parse_range("bytes=-3", 10).unwrap(), (7, 9));
        assert!(parse_range("bytes=10-11", 10).is_err());
    }

    async fn response_json(response: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn filesystem_root_and_home_routes_list_directories() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        let app = build_router(AppState::new(frontend));

        let root_listing = app
            .clone()
            .oneshot(Request::get("/api/fs/tree").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(root_listing.status(), StatusCode::OK);
        assert_eq!(response_json(root_listing).await["path"], "/");

        let home_listing = app
            .oneshot(Request::get("/api/fs/home").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(home_listing.status(), StatusCode::OK);
        assert_eq!(
            response_json(home_listing).await["path"],
            PROCESS_HOME.to_string_lossy().as_ref()
        );
    }

    #[tokio::test]
    async fn pull_request_routes_are_read_only_and_require_a_repository() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        let app = build_router(AppState::new(frontend));

        for path in [
            "/api/my-pull-requests",
            "/api/my-pull-requests/1",
            "/api/my-pull-requests/1/diff",
        ] {
            let response = app
                .clone()
                .oneshot(Request::post(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
        }

        let missing_repository = app
            .oneshot(
                Request::get("/api/my-pull-requests")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_repository.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn configured_pin_protects_api_routes_and_invalidates_changed_pin_sessions() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        let state_dir = root.path().join("state");
        std::fs::create_dir(&state_dir).unwrap();
        let pin_path = state_dir.join(auth::PIN_HASH_FILE);
        std::fs::write(&pin_path, format!("{:x}\n", md5::compute(b"123456"))).unwrap();

        let mut state = AppState::new(frontend);
        state.auth = auth::PinAuth::persistent(&state_dir);
        let app = build_router(state);

        let denied = app
            .clone()
            .oneshot(Request::get("/api/fs/tree").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

        let logged_in = app
            .clone()
            .oneshot(
                Request::post("/api/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"pin":"123456"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logged_in.status(), StatusCode::OK);
        let cookie = logged_in.headers().get("set-cookie").unwrap().clone();

        let allowed = app
            .clone()
            .oneshot(
                Request::get("/api/fs/tree")
                    .header("cookie", cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);

        std::fs::write(&pin_path, format!("{:x}\n", md5::compute(b"654321"))).unwrap();
        let invalidated = app
            .oneshot(
                Request::get("/api/fs/tree")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalidated.status(), StatusCode::UNAUTHORIZED);
    }

    fn git(repo: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[tokio::test]
    async fn git_sync_routes_pull_and_push_the_requested_worktree() {
        let root = TempDir::new().unwrap();
        let remote = root.path().join("remote.git");
        let source = root.path().join("source");
        let clone = root.path().join("clone");
        let worktree = root.path().join("linked worktree");
        git(
            root.path(),
            &[
                "init",
                "--bare",
                "-q",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ],
        );
        git(
            root.path(),
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                source.to_str().unwrap(),
            ],
        );
        git(&source, &["config", "user.name", "Sync Test"]);
        git(&source, &["config", "user.email", "sync@example.com"]);
        std::fs::write(source.join("file.txt"), "initial\n").unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-q", "-m", "initial"]);
        git(&source, &["push", "-q", "-u", "origin", "main"]);
        git(
            root.path(),
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        git(
            &clone,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "checkout",
                worktree.to_str().unwrap(),
                "origin/main",
            ],
        );
        git(&worktree, &["config", "user.name", "Sync Test"]);
        git(&worktree, &["config", "user.email", "sync@example.com"]);
        git(&worktree, &["config", "push.default", "upstream"]);
        git(&worktree, &["config", "pull.ff", "only"]);

        let app = build_router(AppState::new(root.path().join("frontend")));
        let request = |command: &str, repo: &Path| {
            Request::post(format!("/api/git/{command}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::json!({ "repo": repo }).to_string()))
                .unwrap()
        };
        std::fs::write(source.join("file.txt"), "from remote\n").unwrap();
        git(&source, &["commit", "-q", "-am", "remote update"]);
        git(&source, &["push", "-q"]);
        let response = app
            .clone()
            .oneshot(request("pull", &worktree))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            std::fs::read_to_string(worktree.join("file.txt")).unwrap(),
            "from remote\n"
        );
        assert_eq!(
            std::fs::read_to_string(clone.join("file.txt")).unwrap(),
            "initial\n"
        );

        std::fs::write(worktree.join("file.txt"), "from worktree\n").unwrap();
        git(&worktree, &["commit", "-q", "-am", "worktree update"]);
        let response = app
            .clone()
            .oneshot(request("push", &worktree))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        git(&source, &["pull", "-q", "--ff-only"]);
        assert_eq!(
            std::fs::read_to_string(source.join("file.txt")).unwrap(),
            "from worktree\n"
        );

        // A diverged remote must reject an ordinary push, preserving both histories.
        std::fs::write(source.join("file.txt"), "new remote change\n").unwrap();
        git(&source, &["commit", "-q", "-am", "remote advances"]);
        git(&source, &["push", "-q"]);
        std::fs::write(worktree.join("file.txt"), "local change\n").unwrap();
        git(&worktree, &["commit", "-q", "-am", "local advances"]);
        let response = app
            .clone()
            .oneshot(request("push", &worktree))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error = response_json(response).await;
        assert_eq!(error["code"], "git_command_failed");
        assert!(error["message"].as_str().unwrap().contains("rejected"));
        let response = app
            .clone()
            .oneshot(request("pull", &worktree))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        for command in ["pull", "push"] {
            let response = app
                .clone()
                .oneshot(request(command, Path::new("relative")))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response_json(response).await["code"], "path_not_absolute");
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/api/git/{command}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        }
    }

    #[tokio::test]
    async fn ignored_route_returns_only_ignored_candidates() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        let repo = root.path().join("ignored repo");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join(".gitignore"), "generated/\n").unwrap();
        std::fs::create_dir(repo.join("generated")).unwrap();
        std::fs::create_dir(repo.join("visible")).unwrap();
        let app = build_router(AppState::new(frontend));
        let body = serde_json::json!({
            "root": repo,
            "paths": [repo.join("generated"), repo.join("visible"), repo.join(".git")]
        });

        let response = app
            .oneshot(
                Request::post("/api/git/ignored")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let response = response_json(response).await;
        assert_eq!(response["repository"], repo.to_string_lossy().as_ref());
        assert_eq!(
            response["ignored"],
            serde_json::json!([repo.join("generated")])
        );
    }

    #[tokio::test]
    async fn commit_detail_route_returns_commit_json() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        let repo = root.path().join("detail repo");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Route Test"]);
        git(&repo, &["config", "user.email", "route@example.com"]);
        std::fs::write(repo.join("file.txt"), "detail\n").unwrap();
        git(&repo, &["add", "file.txt"]);
        git(&repo, &["commit", "-q", "-m", "route detail"]);
        let commit = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(commit.status.success());
        let commit = String::from_utf8(commit.stdout).unwrap();
        let app = build_router(AppState::new(frontend));
        let repo = repo.to_string_lossy().replace(' ', "%20");
        let response = app
            .oneshot(
                Request::get(format!(
                    "/api/git/commit/detail?repo={repo}&commit={}",
                    commit.trim()
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["subject"], "route detail");
        assert_eq!(body["stats"]["files_changed"], 1);
        assert_eq!(body["refs"], serde_json::json!(["HEAD", "main"]));
    }

    #[tokio::test]
    async fn router_reads_writes_ranges_and_rejects_stale_versions() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        let file = root.path().join("sample %20.txt");
        std::fs::write(&file, "abcdef").unwrap();
        let encoded = encode_absolute(&file.to_string_lossy());
        let app = build_router(AppState::new(frontend));

        let listing = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/fs/tree{}",
                    encode_absolute(&root.path().to_string_lossy())
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(listing.status(), StatusCode::OK);
        let listing = response_json(listing).await;
        assert!(
            listing["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == "sample %20.txt")
        );

        let read = app
            .clone()
            .oneshot(
                Request::get(format!("/api/fs/text{encoded}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(read.status(), StatusCode::OK);
        let etag = read.headers()[ETAG].to_str().unwrap().to_owned();
        assert_eq!(response_json(read).await["content"], "abcdef");

        let range = app
            .clone()
            .oneshot(
                Request::get(format!("/api/fs/raw{encoded}"))
                    .header(RANGE, "bytes=1-3")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(range.headers()[CONTENT_RANGE], "bytes 1-3/6");
        assert_eq!(&to_bytes(range.into_body(), 16).await.unwrap()[..], b"bcd");

        let write = app
            .clone()
            .oneshot(
                Request::put(format!("/api/fs/file{encoded}"))
                    .header(IF_MATCH, &etag)
                    .body(Body::from("updated"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(write.status(), StatusCode::OK);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "updated");

        let stale = app
            .clone()
            .oneshot(
                Request::put(format!("/api/fs/file{encoded}"))
                    .header(IF_MATCH, etag)
                    .body(Body::from("stale"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::PRECONDITION_FAILED);
    }

    #[tokio::test]
    async fn upload_route_can_keep_both_files_without_changing_default_writes() {
        let root = TempDir::new().unwrap();
        let file = root.path().join("报告 #1.png");
        std::fs::write(&file, "original").unwrap();
        let app = build_router(AppState::new(root.path().to_path_buf()));
        let encoded = encode_absolute(&file.to_string_lossy());
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/api/fs/file{encoded}?keep_both=true"))
                    .header(CONTENT_TYPE, "image/png")
                    .body(Body::from(vec![0, 1, 2, 255]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let payload = response_json(response).await;
        let uploaded = payload["path"].as_str().unwrap();
        assert_eq!(
            uploaded,
            root.path().join("报告 #1 (1).png").to_string_lossy()
        );
        assert_eq!(std::fs::read(uploaded).unwrap(), vec![0, 1, 2, 255]);
        assert_eq!(std::fs::read(&file).unwrap(), b"original");
        let response = app
            .oneshot(
                Request::put(format!("/api/fs/file{encoded}"))
                    .body(Body::from("updated"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(std::fs::read(&file).unwrap(), b"updated");
    }

    #[tokio::test]
    async fn rename_route_renames_without_overwriting() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        let source = root.path().join("before.txt");
        let existing = root.path().join("existing.txt");
        std::fs::write(&source, "content").unwrap();
        std::fs::write(&existing, "keep").unwrap();
        let app = build_router(AppState::new(frontend));

        let response = app
            .clone()
            .oneshot(
                Request::patch(format!(
                    "/api/fs/file{}",
                    encode_absolute(&source.to_string_lossy())
                ))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"after.ts"}"#))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["name"], "after.ts");
        assert_eq!(
            body["path"],
            root.path().join("after.ts").to_string_lossy().as_ref()
        );
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(root.path().join("after.ts")).unwrap(),
            "content"
        );

        let conflict = app
            .clone()
            .oneshot(
                Request::patch(format!(
                    "/api/fs/file{}",
                    encode_absolute(&root.path().join("after.ts").to_string_lossy())
                ))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"existing.txt"}"#))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(conflict.status(), StatusCode::CONFLICT);
        assert_eq!(response_json(conflict).await["code"], "destination_exists");
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep");

        let invalid = app
            .oneshot(
                Request::patch(format!(
                    "/api/fs/file{}",
                    encode_absolute(&root.path().join("after.ts").to_string_lossy())
                ))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"../escape.ts"}"#))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_json(invalid).await["code"], "invalid_file_name");
    }

    #[tokio::test]
    async fn filesystem_entry_routes_create_rename_and_delete_files_and_directories() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        std::fs::create_dir(&workspace).unwrap();
        let app = build_router(AppState::new(frontend));

        for (name, kind) in [("notes.txt", "file"), ("design", "directory")] {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/api/fs/entries")
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "parent": workspace.to_string_lossy(),
                                "name": name,
                                "kind": kind,
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            let body = response_json(response).await;
            assert_eq!(body["name"], name);
            assert_eq!(body["kind"], kind);
        }
        assert!(workspace.join("notes.txt").is_file());
        assert!(workspace.join("design").is_dir());

        let rename = app
            .clone()
            .oneshot(
                Request::patch("/api/fs/entries")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "path": workspace.join("design").to_string_lossy(),
                            "name": "plans",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rename.status(), StatusCode::OK);
        assert!(workspace.join("plans").is_dir());

        let unsafe_delete = app
            .clone()
            .oneshot(
                Request::delete("/api/fs/entries?path=/tmp/..")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unsafe_delete.status(), StatusCode::BAD_REQUEST);

        for path in [workspace.join("notes.txt"), workspace.join("plans")] {
            let response = app
                .clone()
                .oneshot(
                    Request::delete(format!("/api/fs/entries?path={}", path.to_string_lossy()))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            assert!(!path.exists());
        }
    }

    #[tokio::test]
    async fn aow_notes_use_the_shared_filesystem_routes() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        let repository = root.path().join("repo");
        let notes = root.path().join("notes");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::create_dir(&repository).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root></div>").unwrap();
        git(&repository, &["init", "-q", "-b", "main"]);
        let app = build_router(AppState::new(frontend));

        let register = app
            .clone()
            .oneshot(
                Request::post("/api/aow/projects")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "path": repository,
                            "notes_path": notes,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(register.status(), StatusCode::CREATED);
        let project = response_json(register).await;
        let project_id = project["id"].as_str().unwrap();
        assert_eq!(project["notes_path"], notes.to_string_lossy().as_ref());
        assert!(notes.is_dir());
        assert!(!notes.join(".git").exists());

        let temporary = app
            .clone()
            .oneshot(
                Request::post(format!("/api/aow/projects/{project_id}/notes/temporary"))
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"extension":"md"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(temporary.status(), StatusCode::CREATED);
        let temporary = response_json(temporary).await;
        let temporary_path = temporary["path"].as_str().unwrap();
        assert!(temporary_path.starts_with(notes.to_string_lossy().as_ref()));
        let temporary_name = Path::new(temporary_path)
            .file_name()
            .unwrap()
            .to_string_lossy();
        assert!(temporary_name.starts_with(".tmp-"));
        assert!(temporary_path.ends_with(".md"));
        assert_eq!(temporary_name.len(), 16);

        let listing = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/fs/tree{}",
                    encode_absolute(&notes.to_string_lossy())
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(listing.status(), StatusCode::OK);
        let listing = response_json(listing).await;
        assert_eq!(listing["path"], notes.to_string_lossy().as_ref());
        assert!(
            !listing["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == ".git")
        );
        assert!(
            listing["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == temporary_path)
        );

        let read = app
            .clone()
            .oneshot(
                Request::get(format!("/api/fs/text{}", encode_absolute(temporary_path)))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(read.status(), StatusCode::OK);
        let etag = read.headers()[ETAG].to_str().unwrap().to_owned();

        let write = app
            .clone()
            .oneshot(
                Request::put(format!("/api/fs/file{}", encode_absolute(temporary_path)))
                    .header(IF_MATCH, etag)
                    .body(Body::from("# Notes\n"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(write.status(), StatusCode::OK);
        assert_eq!(
            std::fs::read_to_string(temporary_path).unwrap(),
            "# Notes\n"
        );

        let directory = app
            .clone()
            .oneshot(
                Request::post("/api/fs/entries")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "parent": notes,
                            "name": "tasks",
                            "kind": "directory",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(directory.status(), StatusCode::CREATED);

        let file = app
            .clone()
            .oneshot(
                Request::post("/api/fs/entries")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "parent": notes.join("tasks"),
                            "name": "todo.txt",
                            "kind": "file",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(file.status(), StatusCode::CREATED);
        assert_eq!(
            response_json(file).await["path"],
            notes.join("tasks/todo.txt").to_string_lossy().as_ref()
        );

        let rename = app
            .clone()
            .oneshot(
                Request::patch("/api/fs/entries")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "path": notes.join("tasks/todo.txt"),
                            "name": "done.txt",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rename.status(), StatusCode::OK);
        assert_eq!(
            response_json(rename).await["path"],
            notes.join("tasks/done.txt").to_string_lossy().as_ref()
        );

        let delete = app
            .oneshot(
                Request::delete(format!(
                    "/api/fs/entries?path={}",
                    notes.join("tasks").to_string_lossy()
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete.status(), StatusCode::NO_CONTENT);
        assert!(!notes.join("tasks").exists());
    }

    #[tokio::test]
    async fn mobile_entry_is_served_and_ui_selection_survives_redirects() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        std::fs::create_dir(&frontend).unwrap();
        std::fs::write(frontend.join("index.html"), "<div id=root>mobile</div>").unwrap();
        let app = build_router(AppState::new(frontend));
        for path in ["/m", "/m/", "/m?ui=mobile"] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap(),
                "<div id=root>mobile</div>"
            );
        }
        for path in ["/?ui=desktop", "/aow?ui=desktop", "/?root=/tmp&ui=desktop"] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
            assert_eq!(response.headers()[LOCATION], "/aow/?ui=desktop");
        }
        let response = app
            .oneshot(
                Request::get("/missing-mobile-asset.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn aow_is_served_and_workspace_route_is_not_served() {
        let root = TempDir::new().unwrap();
        let frontend = root.path().join("frontend");
        std::fs::create_dir_all(frontend.join("assets")).unwrap();
        std::fs::write(
            frontend.join("index.html"),
            "<div id=\"root\">workspace</div>",
        )
        .unwrap();
        std::fs::write(frontend.join("assets/index-abc123.js"), "console.log('ok')").unwrap();
        let app = build_router(AppState::new(frontend));
        let root_redirect = app
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(root_redirect.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(root_redirect.headers()[LOCATION], "/aow/");

        let aow = app
            .clone()
            .oneshot(Request::get("/aow").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(aow.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(aow.headers()[LOCATION], "/aow/");

        let aow = app
            .clone()
            .oneshot(Request::get("/aow/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(aow.status(), StatusCode::OK);
        assert_eq!(aow.headers()[CACHE_CONTROL], "no-cache");
        assert!(
            String::from_utf8(to_bytes(aow.into_body(), 1024).await.unwrap().to_vec())
                .unwrap()
                .contains("workspace")
        );

        for path in [
            "/aow/tabs/example",
            "/aow/tabs/example/",
            "/aow/tabs/example?ui=mobile",
            "/aow/tabs/terminal/example",
            "/aow/tabs/files?workspace=w&path=%2Ftmp",
            "/aow/tabs/file?workspace=w&path=%2Ftmp%2Freadme.md",
            "/aow/tabs/diff?workspace=w&source=staged",
            "/aow/tabs/pr/custom/42?workspace=w",
            "/aow/tabs/session/codex/example?workspace=w",
            "/aow/tabs/automation/task?workspace=w",
            "/aow/tabs/automation/task/runs/run?workspace=w",
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
            assert!(
                String::from_utf8(to_bytes(response.into_body(), 1024).await.unwrap().to_vec())
                    .unwrap()
                    .contains("workspace")
            );
        }
        let unknown = app
            .clone()
            .oneshot(
                Request::get("/aow/unknown/page")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

        let legacy_workspace = app
            .clone()
            .oneshot(Request::get("/workspace/tmp/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(legacy_workspace.status(), StatusCode::NOT_FOUND);
        let browser = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/fs{}/",
                    encode_absolute(&root.path().to_string_lossy())
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(browser.status(), StatusCode::OK);
        let browser = String::from_utf8(
            to_bytes(browser.into_body(), 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(browser.contains("<th>权限</th>"));
        assert!(browser.contains("Project AOW"));
        assert!(browser.contains("/aow/"));

        let legacy_view = app
            .clone()
            .oneshot(Request::get("/view/tmp/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(legacy_view.status(), StatusCode::NOT_FOUND);

        let legacy_workspace = app
            .clone()
            .oneshot(Request::get("/workspace/tmp").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(legacy_workspace.status(), StatusCode::NOT_FOUND);

        let legacy_query = app
            .clone()
            .oneshot(Request::get("/?root=/tmp").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(legacy_query.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(legacy_query.headers()[LOCATION], "/aow/");

        let asset = app
            .oneshot(
                Request::get("/assets/index-abc123.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(
            asset.headers()[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
    }
}
