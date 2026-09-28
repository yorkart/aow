use crate::aow::resolve_executable;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use super::{Provider, PullRequestError, Result, invalid_json};

pub(super) const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) async fn git(repo: &str, args: &[&str], paths: &[PathBuf]) -> Result<String> {
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
pub(super) async fn run_adapter(
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
pub(super) async fn read_limited(reader: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
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
pub(super) async fn run_command(
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
