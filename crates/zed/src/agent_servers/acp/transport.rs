//! Source: Zed agent_servers/src/acp/transport.rs; Tokio replaces GPUI process/executor services.
use super::debug_log::AcpDebugLog;
use crate::project::agent_server_store::AgentServerCommand;
use anyhow::{Context, Result};
use futures::{
    AsyncBufReadExt, AsyncWriteExt, FutureExt, Sink, StreamExt, future::BoxFuture, io::BufReader,
    stream::BoxStream,
};
use std::{io, pin::Pin, process::Stdio};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub(super) struct StdioProcess {
    pub child: tokio::process::Child,
    pub process_group: ProcessGroup,
    pub incoming: BoxStream<'static, io::Result<String>>,
    pub outgoing: Pin<Box<dyn Sink<String, Error = io::Error> + Send>>,
    pub stderr: BoxFuture<'static, ()>,
    pub debug_log: AcpDebugLog,
}

/// Own the adapter's process group as well as the launcher (for example, npx).
pub(super) struct ProcessGroup(u32);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // spawn_stdio creates a new process group whose ID is the child's PID.
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

pub(super) fn spawn_stdio(
    cwd: &std::path::Path,
    command: AgentServerCommand,
    path: Option<std::ffi::OsString>,
) -> Result<StdioProcess> {
    let mut builder = tokio::process::Command::new(&command.path);
    builder
        .args(command.args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(path) = path {
        builder.env("PATH", path);
    }
    builder.envs(command.env);
    let mut child = builder.spawn().with_context(|| format!(
        "Failed to start ACP adapter executable {} in {}. Check the command and Settings → Environment execution PATH",
        command.path.display(), cwd.display()
    ))?;
    let process_group = ProcessGroup(child.id().context("Missing adapter PID")?);
    let stdout = child.stdout.take().context("Failed to take stdout")?;
    let stdin = child.stdin.take().context("Failed to take stdin")?;
    let stderr = child.stderr.take().context("Failed to take stderr")?;
    let debug_log = AcpDebugLog::default();
    let incoming = BufReader::new(stdout.compat())
        .lines()
        .inspect({
            let log = debug_log.clone();
            move |result| {
                if let Ok(line) = result {
                    log.record_line("incoming", line);
                }
            }
        })
        .boxed();
    let outgoing = Box::pin(futures::sink::unfold(
        (stdin.compat_write(), debug_log.clone()),
        async move |(mut writer, log), line: String| {
            log.record_line("outgoing", &line);
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            Ok::<_, io::Error>((writer, log))
        },
    ));
    let stderr = {
        let log = debug_log.clone();
        async move {
            let mut lines = BufReader::new(stderr.compat()).lines();
            while let Some(Ok(line)) = lines.next().await {
                log.record_line("stderr", &line);
            }
        }
        .boxed()
    };
    Ok(StdioProcess {
        child,
        process_group,
        incoming,
        outgoing,
        stderr,
        debug_log,
    })
}
