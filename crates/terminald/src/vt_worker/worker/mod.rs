mod actor;
#[cfg(test)]
pub(super) use actor::handle_command_error;

use std::{
    io,
    process::Stdio,
    sync::{Arc, Mutex, atomic::AtomicUsize},
};
use tokio::{
    process::Command,
    sync::{mpsc, watch},
    task::JoinHandle,
};

use super::{
    VtWorkerConfig, config::validate_config, session::VtWorkerClient, transport::RpcClient,
};

pub(crate) struct VtWorker {
    client: VtWorkerClient,
    task: Option<JoinHandle<()>>,
}
impl VtWorker {
    pub(crate) async fn start(config: VtWorkerConfig) -> io::Result<Self> {
        validate_config(&config)?;
        #[cfg(target_os = "macos")]
        let capture_stderr = aow_macos_log::requested();
        #[cfg(not(target_os = "macos"))]
        let capture_stderr = false;
        let mut child = Command::new(&config.program)
            .arg(&config.script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if capture_stderr {
                Stdio::piped()
            } else {
                Stdio::inherit()
            })
            .kill_on_drop(true)
            .spawn()?;
        #[cfg(target_os = "macos")]
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(forward_worker_stderr(stderr));
        }
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("VT worker stdin pipe was not created"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("VT worker stdout pipe was not created"))?;
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let (shutdown, shutdown_changed) = watch::channel(false);
        let client = VtWorkerClient {
            sender,
            shutdown: Arc::new(shutdown),
            scrollback: config.scrollback,
            sessions: Arc::new(Mutex::new(Vec::new())),
            cached_snapshot_bytes: Arc::new(AtomicUsize::new(0)),
            pending_write_bytes: Arc::new(AtomicUsize::new(0)),
        };
        let task = tokio::spawn(actor::run_worker_actor(
            child,
            RpcClient::new(
                stdin,
                stdout,
                config.request_timeout,
                client.sessions.clone(),
            ),
            receiver,
            shutdown_changed,
            config.snapshot_interval,
        ));
        Ok(Self {
            client,
            task: Some(task),
        })
    }

    pub(crate) fn client(&self) -> VtWorkerClient {
        self.client.clone()
    }

    pub(crate) fn shutdown_now(&self) {
        self.client.shutdown_now();
    }

    pub(crate) async fn shutdown(mut self) {
        self.shutdown_now();
        let Some(task) = self.task.take() else {
            return;
        };
        // Every actor operation observes the shutdown watch channel, and the
        // final child wait is independently bounded. Awaiting the actor keeps
        // the kill + reap guarantee instead of aborting while it owns Child.
        let _ = task.await;
    }
}

#[cfg(target_os = "macos")]
async fn forward_worker_stderr(stderr: tokio::process::ChildStderr) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    loop {
        line.clear();
        // Bound memory even if the worker emits an unterminated line. The
        // worker owns this pipe; its exit ends the task without an OS thread.
        match (&mut reader).take(4096).read_until(b'\n', &mut line).await {
            Ok(0) => break,
            Ok(_) => {
                tracing::warn!(message = %String::from_utf8_lossy(&line).trim_end(), "VT worker stderr")
            }
            Err(error) => {
                tracing::warn!(%error, "failed to read VT worker stderr");
                break;
            }
        }
    }
}

impl Drop for VtWorker {
    fn drop(&mut self) {
        self.shutdown_now();
        // Dropping a JoinHandle detaches rather than cancels the task. The
        // actor therefore still invalidates sessions and performs its bounded
        // child kill/wait even if the caller future itself was cancelled.
        self.task.take();
    }
}
