//! Local stdio and process-group ownership live exclusively on the execution node.
use crate::AdapterCommand;
use acp_client::AdapterTransport;
use anyhow::{Context, Result};
use futures::{AsyncBufReadExt, AsyncWriteExt, FutureExt, StreamExt, io::BufReader};
use std::{io, path::Path, process::Stdio};
use tokio_util::{
    compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt},
    sync::CancellationToken,
};

struct ProcessGroup(u32);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // The child leads a dedicated group, including npm launchers and their descendants.
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

pub(crate) fn spawn(
    cwd: &Path,
    command: AdapterCommand,
    connection_id: &str,
) -> Result<AdapterTransport> {
    let mut builder = tokio::process::Command::new(&command.path);
    builder
        .args(command.args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(path) = command.execution_path {
        builder.env("PATH", path);
    }
    builder
        .envs(command.env)
        .env_remove("AOW_INSTANCE")
        .env_remove("AOW_SESSION_ID")
        .env("AOW_ACP_CONNECTION_ID", connection_id);
    let mut child = builder.spawn().with_context(|| format!("Failed to start ACP adapter executable {} in {}. Check the command and Settings → Environment execution PATH", command.path.display(), cwd.display()))?;
    let process_group = ProcessGroup(child.id().context("Missing adapter PID")?);
    let stdout = child.stdout.take().context("Failed to take stdout")?;
    let stdin = child.stdin.take().context("Failed to take stdin")?;
    let stderr = child.stderr.take().context("Failed to take stderr")?;
    let outgoing = Box::pin(futures::sink::unfold(
        stdin.compat_write(),
        async move |mut writer, line: String| {
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            Ok::<_, io::Error>(writer)
        },
    ));
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let lifetime = async move {
        tokio::select! { _ = stopped.cancelled() => {}, _ = child.wait() => {} }
        drop(process_group);
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    .boxed();
    Ok(AdapterTransport {
        incoming: BufReader::new(stdout.compat()).lines().boxed(),
        stderr: BufReader::new(stderr.compat()).lines().boxed(),
        outgoing,
        stop,
        lifetime,
    })
}
