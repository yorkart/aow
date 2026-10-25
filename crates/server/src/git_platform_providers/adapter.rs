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

use super::{GitPlatformError, GitPlatformProvider, Result, invalid_json};

pub(super) const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) async fn git(repo: &str, args: &[&str], paths: &[PathBuf]) -> Result<String> {
    let executable = resolve_executable("git", paths).ok_or_else(|| {
        GitPlatformError::Unavailable(
            aow_i18n::text("server.git_not_found_check_settings_environment").into(),
        )
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
    provider: &GitPlatformProvider,
    request: Value,
    cwd: Option<&Path>,
    paths: &[PathBuf],
    script_path: Option<&Path>,
) -> Result<Value> {
    if !super::validation::valid_filename(&provider.id)
        || provider.script.len() > super::SCRIPT_LIMIT
    {
        return Err(super::invalid_json(
            "upload a single .mjs provider using protocol v4",
        ));
    }
    if request["operation"]
        .as_str()
        .is_none_or(|op| op != "describe" && !super::schema::OPERATIONS.contains(&op))
    {
        return Err(super::invalid_json(
            "unsupported Git platform read operation",
        ));
    }
    let executable = resolve_executable("node", paths).ok_or_else(|| {
        GitPlatformError::Unavailable(
            aow_i18n::text("server.node_not_found_check_settings_environment").into(),
        )
    })?;
    let mut command = Command::new(executable);
    // Drafts are isolated from saved scripts, but keep the uploaded filename too.
    let draft;
    let draft_path;
    let path = match script_path {
        Some(path) => path,
        None => {
            draft = tempfile::tempdir()?;
            draft_path = draft.path().join(&provider.id);
            std::fs::write(&draft_path, &provider.script)?;
            &draft_path
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
    if value.get("version").and_then(Value::as_u64) != Some(super::schema::VERSION) {
        return Err(invalid_json(
            "expected version: 4; upload a Node.js .mjs provider using the current Git platform protocol",
        ));
    }
    match (value.get("result"), value.get("error")) {
        (Some(result), None) if result.is_object() => Ok(result.clone()),
        (None, Some(error)) if error["code"].is_string() && error["message"].is_string() => {
            Err(GitPlatformError::Command(format!(
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
                if let Err(error) = stdin.write_all(input).await
                    && error.kind() != std::io::ErrorKind::BrokenPipe
                {
                    return Err(error.into());
                }
                drop(stdin);
                Ok::<_, GitPlatformError>(())
            },
            read_limited(stdout, OUTPUT_LIMIT),
            read_limited(stderr, 64 * 1024),
            async { Ok::<_, GitPlatformError>(child.wait().await?) }
        )?;
        if !status.success() {
            return Err(GitPlatformError::Command(format!(
                "GitPlatformProvider process exited {status}: {}",
                String::from_utf8_lossy(&err).trim()
            )));
        }
        Ok(out)
    })
    .await
    .map_err(|_| GitPlatformError::Timeout)?
}
