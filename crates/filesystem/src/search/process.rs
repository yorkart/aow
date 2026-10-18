use std::{path::PathBuf, process::Stdio, time::Duration};

use aow_protocol::{FileSearchMode, FileSearchRequest, FileSearchResult};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;

use super::{
    arguments::arguments,
    output::{MAX_RESULTS, Results},
};

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("{0}")]
    Invalid(String),
    #[error(
        "未找到 ripgrep。请重新安装完整的 AoW 发布包，或在 AoW 服务所在机器安装：macOS 使用 brew install ripgrep；Debian/Ubuntu 使用 sudo apt install ripgrep。"
    )]
    Unavailable,
    #[error("搜索已取消")]
    Cancelled,
    #[error("搜索失败：{0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn ripgrep() -> Result<PathBuf, SearchError> {
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let bundled = directory.join("rg");
        if bundled.is_file() {
            return Ok(bundled);
        }
    }
    #[cfg(debug_assertions)]
    {
        let development = PathBuf::from(env!("AOW_BUILD_RG"));
        if development.is_file() {
            return Ok(development);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path).filter(|path| path.is_absolute()) {
            let candidate = directory.join("rg");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(SearchError::Unavailable)
}

pub async fn search_files(
    request: FileSearchRequest,
    cancel: CancellationToken,
) -> Result<FileSearchResult, SearchError> {
    let args = arguments(&request)?;
    if cancel.is_cancelled() {
        return Err(SearchError::Cancelled);
    }
    if request.query.trim().is_empty() {
        return Ok(FileSearchResult::default());
    }
    if !tokio::fs::metadata(&request.root).await?.is_dir() {
        return Err(SearchError::Invalid("搜索路径不是目录".into()));
    }
    execute(ripgrep()?, args, request, cancel, Duration::from_secs(15)).await
}

pub(super) async fn execute(
    program: PathBuf,
    args: Vec<String>,
    request: FileSearchRequest,
    cancel: CancellationToken,
    timeout: Duration,
) -> Result<FileSearchResult, SearchError> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(&request.root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SearchError::Unavailable
            } else {
                SearchError::Io(error)
            }
        })?;
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut stderr = child.stderr.take().unwrap();
    let mut diagnostics = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = (&mut stderr).take(4096).read_to_end(&mut bytes).await;
        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        bytes
    });
    let mut results = Results::default();
    let delimiter = if request.mode == FileSearchMode::Name {
        0
    } else {
        b'\n'
    };
    let read = async {
        let mut record = Vec::new();
        loop {
            let buffer = output.fill_buf().await?;
            if buffer.is_empty() {
                return Ok::<_, std::io::Error>(false);
            }
            let end = buffer.iter().position(|byte| *byte == delimiter);
            let size = end.unwrap_or(buffer.len());
            // Bound a single JSON event even for dense matches on a very long line.
            if record.len() + size > 16 * 1024 * 1024 {
                return Ok(true);
            }
            record.extend_from_slice(&buffer[..size]);
            output.consume(size + usize::from(end.is_some()));
            if end.is_some() {
                results.ingest(&record, &request);
                record.clear();
                if results.count >= MAX_RESULTS {
                    return Ok(true);
                }
            }
        }
    };
    let mut outcome = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(SearchError::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Ok(true),
        result = read => result.map_err(SearchError::Io),
    };
    if !matches!(outcome, Ok(false)) {
        let _ = child.start_kill();
    }
    // A process can close stdout before it exits; cancellation still owns that wait.
    let status = tokio::select! {
        status = child.wait() => Some(status),
        _ = cancel.cancelled() => { outcome = Err(SearchError::Cancelled); None },
        _ = tokio::time::sleep_until(deadline) => { if outcome.is_ok() { outcome = Ok(true); } None },
    };
    let status = match status {
        Some(status) => status?,
        None => {
            let _ = child.start_kill();
            child.wait().await?
        }
    };
    let diagnostic = match tokio::time::timeout(Duration::from_secs(1), &mut diagnostics).await {
        Ok(output) => output.unwrap_or_default(),
        Err(_) => {
            diagnostics.abort();
            results.truncated = true;
            Vec::new()
        }
    };
    results.truncated |= outcome?;
    if !results.truncated && !status.success() && status.code() != Some(1) {
        let detail = String::from_utf8_lossy(&diagnostic).trim().to_owned();
        if results.count == 0 {
            return Err(SearchError::Failed(if detail.is_empty() {
                status.to_string()
            } else {
                detail
            }));
        }
        results.truncated = true;
    }
    Ok(results.finish())
}
