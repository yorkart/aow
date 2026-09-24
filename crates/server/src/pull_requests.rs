//! Provider-independent review API. Scripts speak the versioned JSON protocol in
//! frontend/src/features/pr/review-providers.md; platform-specific behavior lives in adapters.
use crate::{AppState, HttpError, aow::resolve_executable};
use aow_config::ConfigRepository;
use aow_protocol::{MyPullRequests, PullRequestDetail, PullRequestDiff};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

const CONFIG: &str = "review-providers.json";
const SCRIPT_LIMIT: usize = 1024 * 1024;
const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const GITHUB_SCRIPT: &str = include_str!("review_adapters/github.py");
// Upgrade only exact previous bundled scripts, preserving customized adapters.
const PREVIOUS_GITHUB_SCRIPT_MD5: &[&str] = &[
    "d1e903f22605a41bf437bf77bc85a981",
    "28fea8f4f7673254cca56a6122bfd0a2",
];

type Result<T> = std::result::Result<T, PullRequestError>;
#[derive(Debug, thiserror::Error)]
pub enum PullRequestError {
    #[error("{0}")]
    Invalid(String),
    #[error("配置已更新，请重新加载后保存。")]
    Conflict,
    #[error("{0}")]
    Unavailable(String),
    #[error("Provider 脚本执行超时")]
    Timeout,
    #[error("{0}")]
    Command(String),
    #[error("Provider 返回数据不符合协议：{0}")]
    InvalidJson(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
impl From<PullRequestError> for HttpError {
    fn from(error: PullRequestError) -> Self {
        let (status, code) = match &error {
            PullRequestError::Invalid(_) => (StatusCode::BAD_REQUEST, "review_provider_invalid"),
            PullRequestError::Conflict => (StatusCode::CONFLICT, "review_provider_conflict"),
            PullRequestError::Unavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "review_provider_unavailable",
            ),
            PullRequestError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "review_provider_timeout"),
            PullRequestError::InvalidJson(_) => {
                (StatusCode::BAD_GATEWAY, "review_provider_invalid_response")
            }
            _ => (StatusCode::BAD_GATEWAY, "review_provider_failed"),
        };
        Self::new(status, code, error.to_string(), None)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub hosts: Vec<String>,
    pub script: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub providers: Vec<Provider>,
}
#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    revision: u64,
    providers: Vec<StoredProvider>,
}
#[derive(Serialize, Deserialize)]
struct StoredProvider {
    id: String,
    name: String,
    enabled: bool,
    hosts: Vec<String>,
    script_file: String,
}
#[derive(Clone)]
pub struct ProviderManager {
    inner: Arc<Mutex<Settings>>,
    config: Option<ConfigRepository>,
}
impl Default for ProviderManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Settings {
                revision: 0,
                providers: vec![Provider {
                    id: "github".into(),
                    name: "GitHub".into(),
                    enabled: true,
                    hosts: vec!["github.com".into()],
                    script: GITHUB_SCRIPT.into(),
                }],
            })),
            config: None,
        }
    }
}
impl ProviderManager {
    pub fn persistent(state_dir: &Path) -> anyhow::Result<Self> {
        let config = ConfigRepository::initialize(state_dir)?;
        let path = config.directory().join(CONFIG);
        let manager = Self {
            config: Some(config),
            ..Self::default()
        };
        match std::fs::read(&path) {
            Ok(bytes) => {
                let doc: Document = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(
                    doc.version == 1,
                    "unsupported review provider config version"
                );
                let mut providers = Vec::new();
                for p in doc.providers {
                    anyhow::ensure!(
                        managed_script_path(&p.script_file),
                        "invalid managed script path"
                    );
                    let script = std::fs::read_to_string(
                        manager
                            .config
                            .as_ref()
                            .unwrap()
                            .directory()
                            .join(&p.script_file),
                    )?;
                    providers.push(Provider {
                        id: p.id,
                        name: p.name,
                        enabled: p.enabled,
                        hosts: p.hosts,
                        script,
                    });
                }
                let mut settings = Settings {
                    revision: doc.revision,
                    providers,
                };
                validate_settings(&settings)?;
                if upgrade_bundled_scripts(&mut settings, PREVIOUS_GITHUB_SCRIPT_MD5)? {
                    manager.persist(&settings)?;
                }
                *manager.inner.lock().unwrap() = settings;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                manager.persist(&manager.settings()?)?;
            }
            Err(e) => return Err(e.into()),
        }
        Ok(manager)
    }
    fn settings(&self) -> Result<Settings> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| PullRequestError::Command("Provider configuration lock failed".into()))?
            .clone())
    }
    fn save(&self, mut next: Settings) -> Result<Settings> {
        for provider in &mut next.providers {
            provider.name = provider.name.trim().into();
            provider.hosts = provider
                .hosts
                .iter()
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
        }
        validate_settings(&next)?;
        let mut current = self
            .inner
            .lock()
            .map_err(|_| PullRequestError::Command("Provider configuration lock failed".into()))?;
        if current.revision != next.revision {
            return Err(PullRequestError::Conflict);
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| PullRequestError::Invalid("configuration revision overflow".into()))?;
        self.persist(&next)?;
        *current = next.clone();
        Ok(next)
    }
    fn persist(&self, settings: &Settings) -> Result<()> {
        let Some(config) = &self.config else {
            return Ok(());
        };
        let save = |path: &Path, bytes: &[u8]| {
            config
                .save(path, bytes)
                .map_err(|e| PullRequestError::Command(e.to_string()))
        };
        let mut providers = Vec::new();
        for p in &settings.providers {
            // Immutable versions: publishing the manifest last cannot expose a
            // half-written script, and concurrent calls keep their snapshot.
            let script_file = format!(
                "review-providers/{:x}.py",
                md5::compute(p.script.as_bytes())
            );
            save(Path::new(&script_file), p.script.as_bytes())?;
            providers.push(StoredProvider {
                id: p.id.clone(),
                name: p.name.clone(),
                enabled: p.enabled,
                hosts: p.hosts.clone(),
                script_file,
            });
        }
        let doc = Document {
            version: 1,
            revision: settings.revision,
            providers,
        };
        save(Path::new(CONFIG), &serde_json::to_vec_pretty(&doc).unwrap())
    }
    pub async fn targets(&self, repo: &str, paths: &[PathBuf]) -> Result<Vec<Target>> {
        let providers = self.settings()?.providers;
        Self::targets_for(repo, paths, &providers).await
    }

    pub async fn commit_links(
        &self,
        repo: &str,
        remote: &str,
        commit: &str,
        paths: &[PathBuf],
    ) -> Result<Option<CommitLinks>> {
        let settings = self.settings()?;
        let Some(target) = Self::targets_for(repo, paths, &settings.providers)
            .await?
            .into_iter()
            .find(|target| target.remote == remote)
        else {
            return Ok(None);
        };
        let provider = settings
            .providers
            .iter()
            .find(|p| p.id == target.provider)
            .unwrap();
        let description = self
            .run_provider(
                provider,
                json!({"version":2,"operation":"describe","repository":null,"params":{}}),
                None,
                paths,
            )
            .await?;
        let operations = description
            .get("operations")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_json("describe requires operations"))?;
        if !operations
            .iter()
            .any(|op| op.as_str() == Some("commit_links"))
        {
            return Ok(None);
        }
        let result = self.run_provider(
            provider,
            json!({"version":2,"operation":"commit_links",
                "repository":{"root":repo,"host":target.host,"path":target.repository,"remote":target.remote},
                "params":{"commit":commit}}),
            Some(Path::new(repo)),
            paths,
        ).await?;
        Ok(Some(CommitLinks {
            remote_url: web_link(&result, "remote_url")?,
            commit_url: web_link(&result, "commit_url")?,
        }))
    }

    async fn run_provider(
        &self,
        provider: &Provider,
        request: Value,
        cwd: Option<&Path>,
        paths: &[PathBuf],
    ) -> Result<Value> {
        let script_path = self.config.as_ref().map(|c| {
            c.directory().join(format!(
                "review-providers/{:x}.py",
                md5::compute(provider.script.as_bytes())
            ))
        });
        run_adapter(provider, request, cwd, paths, script_path.as_deref()).await
    }

    async fn targets_for(
        repo: &str,
        paths: &[PathBuf],
        providers: &[Provider],
    ) -> Result<Vec<Target>> {
        absolute(repo)?;
        let names = git(repo, &["remote"], paths).await?;
        let mut targets: Vec<Target> = Vec::new();
        for remote in names.lines().filter(|s| !s.is_empty()) {
            let url = git(repo, &["remote", "get-url", "--", remote], paths).await?;
            let Some((host, repository)) = parse_remote(url.trim()) else {
                continue;
            };
            let Some(provider) = providers
                .iter()
                .find(|p| p.enabled && p.hosts.contains(&host))
            else {
                continue;
            };
            targets.push(Target {
                remote: remote.into(),
                host,
                repository,
                provider: provider.id.clone(),
                provider_name: provider.name.clone(),
            });
        }
        Ok(targets)
    }
    pub async fn call(
        &self,
        query: &ReviewQuery,
        operation: &str,
        params: Value,
        paths: &[PathBuf],
    ) -> Result<Value> {
        let settings = self.settings()?;
        let targets = Self::targets_for(&query.repo, paths, &settings.providers).await?;
        let mut matches: Vec<_> = targets
            .into_iter()
            .filter(|t| {
                query
                    .provider
                    .as_deref()
                    .filter(|p| *p != "auto")
                    .is_none_or(|p| t.provider == p)
                    && query.remote.as_ref().is_none_or(|r| &t.remote == r)
            })
            .collect();
        // Two remote aliases for the same repository do not create ambiguity.
        let mut seen = HashSet::new();
        matches.retain(|t| seen.insert((t.provider.clone(), t.host.clone(), t.repository.clone())));
        let target = match matches.len() {
            0 => return Err(PullRequestError::Unavailable(
                "未找到匹配的 PR Provider，请在 Settings → Pull Requests 配置 remote 域名和脚本。"
                    .into(),
            )),
            1 => matches.remove(0),
            _ => {
                return Err(PullRequestError::Invalid(
                    "多个 remote 匹配 PR Provider，请先选择 remote。".into(),
                ));
            }
        };
        let provider = settings
            .providers
            .into_iter()
            .find(|p| p.id == target.provider && p.enabled)
            .ok_or_else(|| PullRequestError::Unavailable("Provider 已停用，请刷新。".into()))?;
        let request = json!({"version":2,"operation":operation,
            "repository":{"root":query.repo,"host":target.host,"path":target.repository,"remote":target.remote},"params":params});
        let result = self
            .run_provider(&provider, request, Some(Path::new(&query.repo)), paths)
            .await?;
        let mut result = match operation {
            "list" => {
                let mut data: MyPullRequests = decode(result)?;
                data.repository = query.repo.clone();
                for pr in &data.pull_requests {
                    positive(pr.number).map_err(invalid_json)?;
                }
                serde_json::to_value(data).unwrap()
            }
            "detail" => {
                let data: PullRequestDetail = decode(result)?;
                if Some(data.summary.number) != params["number"].as_u64() {
                    return Err(invalid_json("detail number differs from request"));
                }
                for file in &data.files {
                    safe_path(&file.path).map_err(invalid_json)?;
                }
                serde_json::to_value(data).unwrap()
            }
            "diff" => {
                let mut data: PullRequestDiff = decode(result)?;
                if Some(data.number) != params["number"].as_u64()
                    || Some(data.path.as_str()) != params["path"].as_str()
                {
                    return Err(invalid_json("diff identity differs from request"));
                }
                if let Some(path) = &data.original_path {
                    safe_path(path).map_err(invalid_json)?;
                }
                data.repository = query.repo.clone();
                serde_json::to_value(data).unwrap()
            }
            _ => return Err(PullRequestError::Invalid("unknown operation".into())),
        };
        let attach = |value: &mut Value| {
            value["provider"] = json!(target.provider);
            value["provider_name"] = json!(target.provider_name);
            value["remote"] = json!(target.remote);
        };
        attach(&mut result);
        if let Some(items) = result
            .get_mut("pull_requests")
            .and_then(Value::as_array_mut)
        {
            for item in items {
                attach(item);
            }
        }
        Ok(result)
    }
}
fn upgrade_bundled_scripts(settings: &mut Settings, previous_digests: &[&str]) -> Result<bool> {
    let mut changed = false;
    for provider in &mut settings.providers {
        if previous_digests
            .contains(&format!("{:x}", md5::compute(provider.script.as_bytes())).as_str())
        {
            provider.script = GITHUB_SCRIPT.into();
            changed = true;
        }
    }
    if changed {
        settings.revision = settings
            .revision
            .checked_add(1)
            .ok_or_else(|| PullRequestError::Invalid("configuration revision overflow".into()))?;
    }
    Ok(changed)
}

pub struct CommitLinks {
    pub remote_url: Option<String>,
    pub commit_url: Option<String>,
}

// Validate clickable links without making assumptions about the hosting platform.
fn web_link(result: &Value, field: &str) -> Result<Option<String>> {
    match result.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let url = reqwest::Url::parse(value).map_err(invalid_json)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || value
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
            {
                return Err(invalid_json(format!(
                    "{field} must be an HTTP(S) URL without credentials"
                )));
            }
            Ok(Some(value.clone()))
        }
        _ => Err(invalid_json(format!(
            "{field} must be a URL string or null"
        ))),
    }
}

fn managed_script_path(path: &str) -> bool {
    let path = Path::new(path);
    path.parent() == Some(Path::new("review-providers"))
        && path.extension().is_some_and(|s| s == "py")
        && path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(valid_id)
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn validate_settings(settings: &Settings) -> Result<()> {
    if settings.providers.len() > 32 {
        return Err(PullRequestError::Invalid(
            "最多配置 32 个 Provider。".into(),
        ));
    }
    let mut ids = HashSet::new();
    let mut hosts = HashSet::new();
    for p in &settings.providers {
        if !valid_id(&p.id) || !ids.insert(&p.id) || p.id == "auto" {
            return Err(PullRequestError::Invalid(
                "Provider ID 必须唯一，只能包含字母、数字、-、_，且不能为 auto。".into(),
            ));
        }
        if p.name.trim().is_empty() || p.script.trim().is_empty() || p.script.len() > SCRIPT_LIMIT {
            return Err(PullRequestError::Invalid(
                "名称和脚本必填；脚本最大 1 MiB。".into(),
            ));
        }
        if p.hosts.is_empty() {
            return Err(PullRequestError::Invalid(
                "请至少配置一个 remote 域名。".into(),
            ));
        }
        for host in &p.hosts {
            if host.is_empty()
                || host.len() > 253
                || !host
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-_:[]".contains(&c))
            {
                return Err(PullRequestError::Invalid(format!(
                    "无效的 remote 域名：{host}"
                )));
            }
            if p.enabled && !hosts.insert(host) {
                return Err(PullRequestError::Invalid(format!(
                    "多个启用的 Provider 匹配同一个域名：{host}"
                )));
            }
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub remote: String,
    pub host: String,
    pub repository: String,
    pub provider: String,
    pub provider_name: String,
}
#[derive(Deserialize)]
pub struct ReviewQuery {
    pub repo: String,
    pub provider: Option<String>,
    pub remote: Option<String>,
}
#[derive(Deserialize)]
pub struct DiffQuery {
    #[serde(flatten)]
    pub target: ReviewQuery,
    pub path: String,
    #[serde(default)]
    pub patch_only: bool,
}
// Parse HTTPS, ssh:// and scp-style remotes. Local paths have no provider.
fn parse_remote(raw: &str) -> Option<(String, String)> {
    let (host, path) = if raw.contains("://") {
        let url = reqwest::Url::parse(raw).ok()?;
        if !matches!(url.scheme(), "https" | "http" | "ssh" | "git") {
            return None;
        }
        let mut host = url.host_str()?.to_ascii_lowercase();
        if let Some(port) = url
            .port()
            .filter(|_| matches!(url.scheme(), "https" | "http"))
        {
            host.push_str(&format!(":{port}"));
        }
        (host, url.path().trim_start_matches('/').to_string())
    } else {
        let (authority, path) = raw.split_once(':')?;
        if authority.contains('/') || authority.is_empty() {
            return None;
        }
        (
            authority.rsplit('@').next()?.to_ascii_lowercase(),
            path.trim_start_matches('/').to_string(),
        )
    };
    let path = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_end_matches('/'))
        .to_string();
    if host.is_empty() || path.is_empty() || !path.contains('/') {
        return None;
    }
    Some((host, path))
}
fn absolute(repo: &str) -> Result<()> {
    if !Path::new(repo).is_absolute() {
        return Err(PullRequestError::Invalid(
            "repository path must be absolute".into(),
        ));
    }
    Ok(())
}
pub fn positive(number: u64) -> Result<()> {
    if number == 0 {
        Err(PullRequestError::Invalid(
            "PR number must be positive".into(),
        ))
    } else {
        Ok(())
    }
}
pub fn safe_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(PullRequestError::Invalid(
            "file path must be repository-relative".into(),
        ));
    }
    Ok(())
}
fn invalid_json(message: impl ToString) -> PullRequestError {
    PullRequestError::InvalidJson(message.to_string())
}
fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(invalid_json)
}
async fn git(repo: &str, args: &[&str], paths: &[PathBuf]) -> Result<String> {
    let executable = resolve_executable("git", paths).ok_or_else(|| {
        PullRequestError::Unavailable("找不到 git，请检查 Settings → Environment。".into())
    })?;
    let mut command = Command::new(executable);
    command.args(args).current_dir(repo);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(key);
    }
    String::from_utf8(run_command(command, &[], paths, COMMAND_TIMEOUT).await?)
        .map_err(invalid_json)
}
async fn run_adapter(
    provider: &Provider,
    request: Value,
    cwd: Option<&Path>,
    paths: &[PathBuf],
    script_path: Option<&Path>,
) -> Result<Value> {
    let executable = resolve_executable("python3", paths).ok_or_else(|| {
        PullRequestError::Unavailable("找不到 python3，请检查 Settings → Environment。".into())
    })?;
    let mut command = Command::new(executable);
    // Draft tests and in-memory embeddings use a temporary file. Saved providers
    // execute their immutable managed file directly.
    let draft;
    let path = match script_path {
        Some(path) => path,
        None => {
            use std::io::Write;
            let mut file = tempfile::NamedTempFile::new()?;
            file.write_all(provider.script.as_bytes())?;
            draft = file;
            draft.path()
        }
    };
    command.arg(path);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let bytes = run_command(
        command,
        &serde_json::to_vec(&request).unwrap(),
        paths,
        COMMAND_TIMEOUT,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(invalid_json)?;
    if value.get("version").and_then(Value::as_u64) != Some(2) {
        return Err(invalid_json(
            "expected version: 2; update the Provider script to the current PR protocol",
        ));
    }
    match (value.get("result"), value.get("error")) {
        (Some(result), None) if result.is_object() => Ok(result.clone()),
        (None, Some(error)) if error["code"].is_string() && error["message"].is_string() => {
            Err(PullRequestError::Command(format!(
                "{}: {}",
                error["code"].as_str().unwrap(),
                error["message"].as_str().unwrap()
            )))
        }
        _ => Err(invalid_json(
            "expected either result object or error {code, message}",
        )),
    }
}
async fn read_limited(reader: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(invalid_json("process output exceeded limit"));
    }
    Ok(bytes)
}
async fn run_command(
    mut command: Command,
    input: &[u8],
    paths: &[PathBuf],
    deadline: Duration,
) -> Result<Vec<u8>> {
    command
        .env("PATH", std::env::join_paths(paths).map_err(invalid_json)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    // Kill the entire process group even on cancellation/timeout (scripts often
    // spawn CLI children which inherit stdout and would otherwise outlive them).
    struct Group(Option<u32>);
    impl Drop for Group {
        fn drop(&mut self) {
            #[cfg(unix)]
            if let Some(pid) = self.0 {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }
    }
    let _group = Group(child.id());
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    timeout(deadline, async {
        let (_, out, err, status) = tokio::try_join!(
            async {
                if let Err(error) = stdin.write_all(input).await {
                    if error.kind() != std::io::ErrorKind::BrokenPipe {
                        return Err(error.into());
                    }
                }
                drop(stdin);
                Ok::<_, PullRequestError>(())
            },
            read_limited(stdout, OUTPUT_LIMIT),
            read_limited(stderr, 64 * 1024),
            async { Ok::<_, PullRequestError>(child.wait().await?) }
        )?;
        if !status.success() {
            return Err(PullRequestError::Command(format!(
                "Provider process exited {status}: {}",
                String::from_utf8_lossy(&err).trim()
            )));
        }
        Ok(out)
    })
    .await
    .map_err(|_| PullRequestError::Timeout)?
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/aow/review-providers",
            get(settings).put(save_settings),
        )
        .route("/api/aow/review-providers/test", post(test_provider))
        .route("/api/review-targets", get(targets))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
}
async fn settings(State(state): State<AppState>) -> std::result::Result<Json<Settings>, HttpError> {
    Ok(Json(state.review_providers.settings()?))
}
async fn save_settings(
    State(state): State<AppState>,
    Json(settings): Json<Settings>,
) -> std::result::Result<Json<Settings>, HttpError> {
    let manager = state.review_providers.clone();
    let settings = tokio::task::spawn_blocking(move || manager.save(settings))
        .await
        .map_err(|e| PullRequestError::Command(e.to_string()))??;
    Ok(Json(settings))
}
async fn test_provider(
    State(state): State<AppState>,
    Json(provider): Json<Provider>,
) -> std::result::Result<Json<Value>, HttpError> {
    validate_settings(&Settings {
        revision: 0,
        providers: vec![provider.clone()],
    })?;
    let paths = state
        .aow
        .execution_path()
        .await
        .map_err(|e| PullRequestError::Command(e.to_string()))?;
    let result = run_adapter(
        &provider,
        json!({"version":2,"operation":"describe","repository":null,"params":{}}),
        None,
        &paths,
        None,
    )
    .await?;
    let operations = result
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_json("describe requires operations"))?;
    if !["list", "detail", "diff"]
        .iter()
        .all(|op| operations.iter().any(|v| v.as_str() == Some(op)))
    {
        return Err(invalid_json("describe must implement list, detail, diff").into());
    }
    Ok(Json(result))
}
async fn targets(
    State(state): State<AppState>,
    Query(query): Query<ReviewQuery>,
) -> std::result::Result<Json<Vec<Target>>, HttpError> {
    Ok(Json(
        state
            .review_providers
            .targets(
                &query.repo,
                &state
                    .aow
                    .execution_path()
                    .await
                    .map_err(|e| PullRequestError::Command(e.to_string()))?,
            )
            .await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn paths() -> Vec<PathBuf> {
        vec!["/usr/local/bin".into(), "/usr/bin".into(), "/bin".into()]
    }
    fn provider(script: &str) -> Provider {
        Provider {
            id: "custom".into(),
            name: "Custom".into(),
            enabled: true,
            hosts: vec!["git.example.com".into()],
            script: script.into(),
        }
    }
    fn repository() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        let result = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert!(result.status.success());
        repo
    }
    fn remote(repo: &Path, name: &str, url: &str) {
        assert!(
            std::process::Command::new("git")
                .args(["remote", "add", name, url])
                .current_dir(repo)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    #[test]
    fn parses_remote_hosts_without_platform_assumptions() {
        for (raw, host, path) in [
            ("git@github.com:owner/repo.git", "github.com", "owner/repo"),
            (
                "ssh://git@git.example.com:2222/group/sub/repo.git",
                "git.example.com",
                "group/sub/repo",
            ),
            (
                "https://user:secret@Git.Example.com:8443/group/repo.git",
                "git.example.com:8443",
                "group/repo",
            ),
            ("git@work-alias:team/repo.git", "work-alias", "team/repo"),
        ] {
            assert_eq!(parse_remote(raw), Some((host.into(), path.into())));
        }
        for raw in [
            "/local/repo",
            "../repo",
            "file:///tmp/repo",
            "https://example.com",
            "C:/repo",
        ] {
            assert!(parse_remote(raw).is_none(), "{raw}");
        }
    }
    #[test]
    fn defaults_scripts_edits_and_empty_config_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ProviderManager::persistent(dir.path()).unwrap();
        let initial = manager.settings().unwrap();
        assert_eq!(initial.providers.len(), 1);
        assert_eq!(initial.providers[0].id, "github");
        let mut next = initial.clone();
        next.providers[0].script = "print('customized')\n".into();
        next.providers[0].enabled = false;
        manager.save(next).unwrap();
        assert!(matches!(
            manager.save(initial),
            Err(PullRequestError::Conflict)
        ));
        let reloaded = ProviderManager::persistent(dir.path()).unwrap();
        let mut next = reloaded.settings().unwrap();
        assert!(!next.providers[0].enabled);
        assert_eq!(next.providers[0].script, "print('customized')\n");
        let config = reloaded.config.as_ref().unwrap();
        let document: Document =
            serde_json::from_slice(&std::fs::read(config.directory().join(CONFIG)).unwrap())
                .unwrap();
        assert_eq!(
            std::fs::read_to_string(config.directory().join(&document.providers[0].script_file))
                .unwrap(),
            next.providers[0].script
        );
        next.providers.clear();
        reloaded.save(next).unwrap();
        assert!(
            ProviderManager::persistent(dir.path())
                .unwrap()
                .settings()
                .unwrap()
                .providers
                .is_empty()
        );
    }
    #[test]
    fn rejects_conflicting_hosts_and_path_ids() {
        let manager = ProviderManager::default();
        let mut next = manager.settings().unwrap();
        let mut second = next.providers[0].clone();
        second.id = "enterprise".into();
        next.providers.push(second);
        assert!(manager.save(next.clone()).is_err());
        next.providers[1].enabled = false;
        let mut next = manager.save(next).unwrap();
        next.providers[1].id = "../outside".into();
        assert!(manager.save(next).is_err());
        assert!(!managed_script_path("review-providers/../../outside.py"));
    }

    #[test]
    fn bundled_protocol_upgrade_preserves_custom_scripts_and_provider_settings() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ProviderManager::persistent(dir.path()).unwrap();
        let previous = "print('previous bundled adapter')\n";
        let digest = format!("{:x}", md5::compute(previous.as_bytes()));
        let mut settings = manager.settings().unwrap();
        settings.providers[0].script = previous.into();
        settings.providers[0].name = "Personal GitHub".into();
        settings.providers[0].enabled = false;
        let mut custom = settings.providers[0].clone();
        custom.id = "custom".into();
        custom.script.push_str("# local edits\n");
        settings.providers.push(custom.clone());
        let revision = settings.revision;
        assert!(upgrade_bundled_scripts(&mut settings, &["unrelated-digest", &digest]).unwrap());
        assert_eq!(settings.revision, revision + 1);
        assert_eq!(settings.providers[0].script, GITHUB_SCRIPT);
        assert_eq!(settings.providers[0].name, "Personal GitHub");
        assert!(!settings.providers[0].enabled);
        assert_eq!(settings.providers[1].script, custom.script);
        assert!(!upgrade_bundled_scripts(&mut settings, &["unrelated-digest", &digest]).unwrap());
        assert_eq!(settings.revision, revision + 1);
        manager.persist(&settings).unwrap();
        let restored = ProviderManager::persistent(dir.path())
            .unwrap()
            .settings()
            .unwrap();
        assert_eq!(restored.revision, settings.revision);
        assert_eq!(restored.providers[0].script, GITHUB_SCRIPT);
        assert_eq!(restored.providers[1].script, custom.script);
    }
    const FIXTURE: &str = r#"import json,sys,os
r=json.load(sys.stdin)
assert r['version']==2
assert set(r)=={'version','operation','repository','params'}
assert r['repository']['host']=='git.example.com'
assert r['repository']['root']==os.getcwd()
p=r['params']
summary=dict(number=p.get('number',42),status='open',draft=False,title=r['repository']['path'],source_branch='feature',target_branch='main',url=None,created_at='',updated_at='')
if r['operation']=='list':
    result=dict(repository='ignored',current_branch='feature',current_user=dict(id='u',username='user',display_name='User'),pull_requests=[summary])
elif r['operation']=='detail':
    result=dict(summary,description='body',changes_count=1,commits_count=1,review_status='approved',check_summary_status='passed',mergeable=True,reviewers=[],checks=[],unresolved_threads=[],files=[dict(path='file.txt',change_type='M',additions=1,deletions=1)],author=None,labels=[],merge_checks=[],threads=[],warnings=[],diverged_commits_count=0,milestone=None)
else:
    result=dict(repository='ignored',number=p['number'],path=p['path'],original_path=None,original='before',modified='after',patch='@@ -1 +1 @@\n-before\n+after\n',binary=False,truncated=False)
print(json.dumps(dict(version=2,result=result)))
"#;
    #[tokio::test]
    async fn custom_provider_routes_all_operations_and_requires_unambiguous_remote() {
        let repo = repository();
        remote(repo.path(), "origin", "git@git.example.com:team/one.git");
        let manager = ProviderManager::default();
        manager
            .save(Settings {
                revision: 0,
                providers: vec![provider(FIXTURE)],
            })
            .unwrap();
        let mut query = ReviewQuery {
            repo: repo.path().to_string_lossy().into_owned(),
            provider: None,
            remote: None,
        };
        let result = manager
            .call(&query, "list", json!({}), &paths())
            .await
            .unwrap();
        assert_eq!(result["provider"], "custom");
        assert_eq!(result["repository"], query.repo);
        assert_eq!(result["pull_requests"][0]["remote"], "origin");
        // Exercise the public routes as well as the adapter so their names and
        // serialized response fields cannot drift away from the frontend API.
        let mut state = AppState::new(PathBuf::new());
        state.review_providers = manager.clone();
        let app = crate::build_router(state);
        for suffix in ["", "/42", "/42/diff"] {
            use axum::{
                body::{Body, to_bytes},
                http::Request,
            };
            use tower::ServiceExt;
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!(
                        "/api/my-pull-requests{suffix}?repo={}&path=file.txt",
                        query.repo
                    ))
                    .body(Body::empty())
                    .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{suffix}");
            let body = to_bytes(response.into_body(), OUTPUT_LIMIT).await.unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["provider"], "custom");
            if suffix.is_empty() {
                assert_eq!(value["pull_requests"][0]["number"], 42);
                assert_eq!(value["pull_requests"][0]["remote"], "origin");
            } else {
                assert_eq!(value["number"], 42);
            }
        }
        let detail = manager
            .call(&query, "detail", json!({"number":42}), &paths())
            .await
            .unwrap();
        assert_eq!(detail["title"], "team/one");
        let diff = manager
            .call(
                &query,
                "diff",
                json!({"number":42,"path":"file.txt","patch_only":false}),
                &paths(),
            )
            .await
            .unwrap();
        assert_eq!(diff["modified"], "after");
        remote(
            repo.path(),
            "mirror",
            "https://git.example.com/team/one.git",
        );
        manager
            .call(&query, "list", json!({}), &paths())
            .await
            .unwrap();
        remote(
            repo.path(),
            "upstream",
            "https://git.example.com/team/two.git",
        );
        assert!(matches!(
            manager.call(&query, "list", json!({}), &paths()).await,
            Err(PullRequestError::Invalid(_))
        ));
        query.remote = Some("upstream".into());
        query.provider = Some("custom".into());
        assert_eq!(
            manager
                .call(&query, "list", json!({}), &paths())
                .await
                .unwrap()["pull_requests"][0]["title"],
            "team/two"
        );
        query.provider = Some("missing".into());
        assert!(matches!(
            manager.call(&query, "list", json!({}), &paths()).await,
            Err(PullRequestError::Unavailable(_))
        ));
    }
    #[tokio::test]
    async fn managed_script_executes_after_restart() {
        let repo = repository();
        remote(repo.path(), "origin", "git@git.example.com:team/one.git");
        let state = tempfile::tempdir().unwrap();
        let manager = ProviderManager::persistent(state.path()).unwrap();
        manager
            .save(Settings {
                revision: 0,
                providers: vec![provider(FIXTURE)],
            })
            .unwrap();
        let manager = ProviderManager::persistent(state.path()).unwrap();
        let query = ReviewQuery {
            repo: repo.path().to_string_lossy().into_owned(),
            provider: None,
            remote: None,
        };
        assert_eq!(
            manager
                .call(&query, "list", json!({}), &paths())
                .await
                .unwrap()["provider"],
            "custom"
        );
    }

    const LINKS_FIXTURE: &str = r#"import json,sys,os
r=json.load(sys.stdin)
assert r['version']==2
assert 'secret' not in json.dumps(r)
if r['operation']=='describe':
    assert r['repository'] is None
    result={'operations':['list','detail','diff','commit_links']}
else:
    assert r['operation']=='commit_links'
    assert r['repository']==dict(root=os.getcwd(),host='git.example.com',path='team/nested/project',remote='review')
    sha=r['params']['commit']
    assert len(sha)==40 and all(c in '0123456789abcdef' for c in sha)
    result={'remote_url':'https://browser.example.org/project/123',
            'commit_url':'https://browser.example.org/project/123/revisions/'+sha}
print(json.dumps(dict(version=2,result=result)))
"#;

    #[tokio::test]
    async fn commit_links_follow_configured_provider_and_preserve_local_details() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;

        let repo = repository();
        let root = repo.path().canonicalize().unwrap();
        assert!(
            std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Example",
                    "-c",
                    "user.email=example@example.com",
                    "commit",
                    "--quiet",
                    "--allow-empty",
                    "-m",
                    "Local commit"
                ])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        remote(
            &root,
            "origin",
            "git@unconfigured.example.com:other/project.git",
        );
        remote(
            &root,
            "review",
            "ssh://user:secret@git.example.com:2222/team/nested/project.git",
        );
        let branch = git(
            root.to_str().unwrap(),
            &["branch", "--show-current"],
            &paths(),
        )
        .await
        .unwrap();
        git(
            root.to_str().unwrap(),
            &[
                "config",
                &format!("branch.{}.remote", branch.trim()),
                "review",
            ],
            &paths(),
        )
        .await
        .unwrap();
        let sha = git(root.to_str().unwrap(), &["rev-parse", "HEAD"], &paths())
            .await
            .unwrap();
        let sha = sha.trim();
        let manager = ProviderManager::default();
        let mut state = AppState::new(PathBuf::new());
        state.review_providers = manager.clone();
        let app = crate::build_router(state);
        let url = format!(
            "/api/git/commit/detail?repo={}&commit={}",
            root.display(),
            &sha[..10]
        );

        for (script, enabled, has_links) in [
            (LINKS_FIXTURE, true, true),
            (LINKS_FIXTURE, false, false),
            (
                "import json;print(json.dumps({'version':2,'result':{'operations':['list','detail','diff']}}))",
                true,
                false,
            ),
            ("raise RuntimeError('Provider failed')", true, false),
            ("import time;time.sleep(60)", true, false),
        ] {
            let mut settings = manager.settings().unwrap();
            settings.providers = vec![Provider {
                enabled,
                ..provider(script)
            }];
            manager.save(settings).unwrap();
            let response = app
                .clone()
                .oneshot(Request::get(&url).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let value: Value = serde_json::from_slice(
                &to_bytes(response.into_body(), OUTPUT_LIMIT).await.unwrap(),
            )
            .unwrap();
            assert_eq!(value["id"], sha);
            assert_eq!(value["subject"], "Local commit");
            assert_eq!(value["remote_name"], "review");
            if has_links {
                assert_eq!(
                    value["remote_url"],
                    "https://browser.example.org/project/123"
                );
                assert_eq!(
                    value["commit_url"],
                    format!("https://browser.example.org/project/123/revisions/{sha}")
                );
            } else {
                assert!(value["remote_url"].is_null());
                assert!(value["commit_url"].is_null());
            }
        }

        let mut settings = manager.settings().unwrap();
        settings.providers = vec![provider(LINKS_FIXTURE)];
        manager.save(settings).unwrap();
        // A configured provider for a different remote must not replace the
        // local branch's selected remote or invent a URL for it.
        assert!(
            manager
                .commit_links(root.to_str().unwrap(), "origin", sha, &paths())
                .await
                .unwrap()
                .is_none()
        );
        let mut settings = manager.settings().unwrap();
        settings.providers.clear();
        manager.save(settings).unwrap();
        assert!(
            manager
                .commit_links(root.to_str().unwrap(), "review", sha, &paths())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn commit_links_accept_only_explicit_web_urls_or_null() {
        for url in [
            "https://web.example.com/project/revision/abc",
            "http://localhost:8080/changes/abc",
        ] {
            assert_eq!(
                web_link(&json!({"commit_url":url}), "commit_url")
                    .unwrap()
                    .as_deref(),
                Some(url)
            );
        }
        assert!(
            web_link(&json!({"commit_url":null}), "commit_url")
                .unwrap()
                .is_none()
        );
        for value in [
            json!({}),
            json!({"commit_url":12}),
            json!({"commit_url":"javascript:alert(1)"}),
            json!({"commit_url":"/relative/path"}),
            json!({"commit_url":"https://user:secret@web.example.com/commit"}),
            json!({"commit_url":"https://web.example.com/commit\n"}),
        ] {
            assert!(web_link(&value, "commit_url").is_err(), "{value}");
        }
    }

    #[tokio::test]
    async fn validates_envelope_exit_code_schema_and_process_limits() {
        let request = json!({"version":2,"operation":"describe","params":{}});
        for script in [
            "print('not-json')",
            "print('{\"version\":1,\"result\":{}}')",
            "print('{\"version\":2,\"result\":[],\"error\":{}}')",
            "import sys;sys.exit(1)",
            "print('{\"version\":2,\"error\":{\"code\":\"auth\",\"message\":\"login required\"}}')",
        ] {
            assert!(
                run_adapter(&provider(script), request.clone(), None, &paths(), None)
                    .await
                    .is_err(),
                "{script}"
            );
        }
        assert!(decode::<MyPullRequests>(json!({"pull_requests":[]})).is_err());
        assert!(
            decode::<MyPullRequests>(json!({
                "repository":"/repo", "current_branch":"main",
                "current_user":{"id":"u","username":"user","display_name":"User"}
            }))
            .is_err()
        );
        assert!(safe_path("../outside").is_err());
        assert!(positive(0).is_err());
        let mut command = Command::new("/bin/sleep");
        command.arg("10");
        assert!(matches!(
            run_command(command, &[], &paths(), Duration::from_millis(30)).await,
            Err(PullRequestError::Timeout)
        ));
        let reader = &b"12345"[..];
        assert!(read_limited(reader, 4).await.is_err());
    }
    #[tokio::test]
    async fn settings_and_draft_validation_http_contract() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;
        let app = crate::build_router(AppState::new(PathBuf::new()));
        let response = app
            .clone()
            .oneshot(
                Request::get("/api/aow/review-providers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 2 * SCRIPT_LIMIT)
            .await
            .unwrap();
        let mut settings: Settings = serde_json::from_slice(&body).unwrap();
        let settings_json: Value = serde_json::from_slice(&body).unwrap();
        assert!(settings_json["providers"][0].get("cli").is_none());
        let mut draft = settings.providers[0].clone();
        draft.script = r#"import json,sys
r=json.load(sys.stdin)
assert set(r)=={'version','operation','repository','params'}
assert r['operation']=='describe' and r['repository'] is None
print(json.dumps(dict(version=2,result=dict(operations=['list','detail','diff']))))
"#
        .into();
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/aow/review-providers/test")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&draft).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        settings.providers.clear();
        let body = serde_json::to_vec(&settings).unwrap();
        for status in [StatusCode::OK, StatusCode::CONFLICT] {
            let response = app
                .clone()
                .oneshot(
                    Request::put("/api/aow/review-providers")
                        .header("content-type", "application/json")
                        .body(Body::from(body.clone()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
        }
    }
}
