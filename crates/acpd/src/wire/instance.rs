//! A managed stdio adapter, with bounded queues and deterministic process-group cleanup.

use super::*;
use futures_util::StreamExt;
use std::{
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::codec::{FramedRead, LinesCodec};

pub(super) const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Serialize)]
pub(super) struct Status {
    running: bool,
    exit_code: Option<i32>,
    error: Option<String>,
}

pub(super) struct Instance {
    pub spec: InstanceSpec,
    pub(super) id: String,
    pid: u32,
    attached: AtomicBool,
    stop: watch::Sender<bool>,
    status: watch::Receiver<Status>,
    input: mpsc::Sender<String>,
    output: std::sync::Mutex<Option<mpsc::Receiver<String>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

/// Dropping a reserved attachment, including a failed upgrade, ends its ACP connection.
pub(super) struct Attachment {
    pub instance: Arc<Instance>,
    pub output: mpsc::Receiver<String>,
}
impl Drop for Attachment {
    fn drop(&mut self) {
        self.instance.attached.store(false, Ordering::Release);
        self.instance.request_stop();
    }
}

impl Instance {
    pub fn spawn(
        id: String,
        spec: InstanceSpec,
        command: aow_acp::AdapterCommand,
        mut runtime_stopped: watch::Receiver<bool>,
    ) -> Result<Self> {
        let mut builder = tokio::process::Command::new(&command.path);
        if let Some(path) = command.execution_path {
            builder.env("PATH", path);
        }
        builder.envs(command.env);
        if let Some(environment) = &spec.execution_environment {
            builder
                .env("HOME", &environment.home)
                .envs(&environment.env);
        }
        builder
            .args(command.args)
            .current_dir(&spec.cwd)
            .env_remove("AOW_INSTANCE")
            .env_remove("AOW_SESSION_ID")
            .env("AOW_ACP_CONNECTION_ID", &id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        let mut child = builder
            .spawn()
            .with_context(|| format!("Start ACP adapter {}", command.path.display()))?;
        let pid = child.id().context("Missing ACP adapter PID")?;
        let group = ProcessGroup(pid);
        let stdin = child.stdin.take().context("Missing ACP adapter stdin")?;
        let stdout = child.stdout.take().context("Missing ACP adapter stdout")?;
        let mut stderr = child.stderr.take().context("Missing ACP adapter stderr")?;
        let (input, mut incoming) = mpsc::channel::<String>(16);
        let (outgoing, output) = mpsc::channel(16);
        let (stop, mut stopped) = watch::channel(false);
        let (status_tx, status) = watch::channel(Status {
            running: true,
            exit_code: None,
            error: None,
        });
        let task = tokio::spawn(async move {
            // Drain diagnostics without leaking adapter credentials or unbounded lines into logs.
            let diagnostics = tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            });
            let mut reader =
                FramedRead::new(stdout, LinesCodec::new_with_max_length(MAX_MESSAGE_BYTES));
            let mut writer = BufWriter::new(stdin);
            let result: Result<()> = async {
                loop {
                    tokio::select! {
                        _ = stopped.changed() => break,
                        _ = runtime_stopped.changed() => break,
                        line = reader.next() => {
                            let Some(line) = line else { break; };
                            let line = line?;
                            if line.trim().is_empty() { continue; }
                            ensure!(valid_message(&line).is_ok(), "Adapter emitted invalid ACP JSON-RPC");
                            tokio::time::timeout(IO_TIMEOUT, outgoing.send(line)).await?
                                .map_err(|_| anyhow::anyhow!("ACP attachment closed"))?;
                        },
                        line = incoming.recv() => {
                            let Some(line) = line else { break; };
                            tokio::time::timeout(IO_TIMEOUT, async {
                                writer.write_all(line.as_bytes()).await?;
                                writer.write_all(b"\n").await?;
                                writer.flush().await
                            }).await??;
                        },
                        result = child.wait() => { result?; break; }
                    }
                }
                Ok(())
            }.await;
            // Kill descendants even when the launcher already exited, then reap the launcher.
            drop(group);
            let _ = child.start_kill();
            let exit = child.wait().await.ok().and_then(|status| status.code());
            diagnostics.abort();
            let _ = diagnostics.await;
            status_tx.send_replace(Status {
                running: false,
                exit_code: exit,
                error: result.err().map(|e| e.to_string()),
            });
        });
        Ok(Self {
            spec,
            id,
            pid,
            attached: AtomicBool::new(false),
            stop,
            status,
            input,
            output: std::sync::Mutex::new(Some(output)),
            task: Mutex::new(Some(task)),
        })
    }

    pub fn info(&self) -> InstanceInfo {
        InstanceInfo {
            id: self.id.clone(),
            agent_id: self.spec.agent_id.clone(),
            cwd: self.spec.cwd.clone(),
            pid: self.pid,
            attached: self.attached.load(Ordering::Acquire),
            status: self.status.borrow().clone(),
            websocket_path: format!("/v1/acp/instances/{}/acp", self.id),
            execution_home: self
                .spec
                .execution_environment
                .as_ref()
                .map(|environment| environment.home.clone()),
        }
    }

    pub fn attach(self: &Arc<Self>) -> Result<Attachment, ApiError> {
        if !self.status.borrow().running {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "ACP instance has exited; create another instance and use session/load".into(),
            ));
        }
        let output = self.output.lock().unwrap().take().ok_or_else(|| {
            ApiError(
                StatusCode::CONFLICT,
                "ACP instance already has a controller".into(),
            )
        })?;
        self.attached.store(true, Ordering::Release);
        Ok(Attachment {
            instance: self.clone(),
            output,
        })
    }

    pub fn request_stop(&self) {
        self.stop.send_replace(true);
    }
    pub async fn stop(&self) {
        self.request_stop();
        let mut task = self.task.lock().await;
        if let Some(handle) = task.as_mut() {
            let _ = handle.await;
        }
        task.take();
    }
    pub async fn send(&self, text: String) -> Result<()> {
        tokio::time::timeout(IO_TIMEOUT, self.input.send(text))
            .await?
            .map_err(|_| anyhow::anyhow!("ACP adapter exited"))
    }
}

struct ProcessGroup(u32);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

/// Validate only the JSON-RPC envelope; ACP capabilities and methods belong to the adapter.
pub(super) fn valid_message(text: &str) -> Result<serde_json::Value, i32> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| -32700)?;
    let id_valid = value
        .get("id")
        .is_none_or(|id| id.is_null() || id.is_string() || id.is_i64() || id.is_u64());
    let request = value.get("method").is_some_and(|m| m.is_string())
        && value.get("result").is_none()
        && value.get("error").is_none();
    let response = value.get("method").is_none()
        && value.get("id").is_some()
        && (value.get("result").is_some() ^ value.get("error").is_some());
    if value.is_object() && value["jsonrpc"] == "2.0" && id_valid && (request ^ response) {
        Ok(value)
    } else {
        Err(-32600)
    }
}
