//! PTY creation, worker threads, and process cleanup.

use super::*;

pub(super) struct SpawnedRuntime {
    pub(super) runtime: Arc<Runtime>,
    pub(super) reader: Option<Box<dyn Read + Send>>,
    pub(super) child: Option<Box<dyn Child + Send + Sync>>,
    pub(super) spawn_permit: Option<SpawnPermit>,
}

pub(super) fn compare_existing(
    id: &str,
    existing: Arc<Runtime>,
    spec: &TerminalRuntimeSpec,
) -> Result<CreateOutcome, TerminaldError> {
    if existing.is_deleted() {
        return Err(TerminaldError::Conflict(id.to_owned()));
    }
    if existing.creation_spec()? == *spec {
        Ok(CreateOutcome::Existing(existing.description()?))
    } else {
        Err(TerminaldError::Conflict(id.to_owned()))
    }
}

pub(super) fn start_runtime_workers(mut spawned: SpawnedRuntime) {
    let reader_runtime = spawned.runtime.clone();
    let mut reader = spawned
        .reader
        .take()
        .expect("spawned runtime reader is present before commit");
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(length) => {
                    if let Err(error) = reader_runtime.append_output(&buffer[..length]) {
                        tracing::warn!(%error, runtime_id = %reader_runtime.id, "terminal output accounting failed");
                        break;
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, runtime_id = %reader_runtime.id, "terminal PTY reader stopped");
                    break;
                }
            }
        }
        reader_runtime.close_output();
    });

    let wait_runtime = spawned.runtime.clone();
    let mut child = spawned
        .child
        .take()
        .expect("spawned runtime child is present before commit");
    thread::spawn(move || match child.wait() {
        Ok(status) => {
            wait_runtime.mark_reaped(TerminalPaneStatus::Exited, Some(status.exit_code()), true)
        }
        Err(error) => {
            tracing::warn!(%error, runtime_id = %wait_runtime.id, "failed waiting for terminal runtime");
            wait_runtime.kill_best_effort();
            wait_runtime.mark_reaped(TerminalPaneStatus::Interrupted, None, false);
        }
    });
    // Both long-lived workers now own the runtime and child. It is visible in
    // the daemon map, so shutdown can account for it through normal draining.
    drop(spawned.spawn_permit.take());
}

impl Drop for SpawnedRuntime {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        self.runtime.notify_deleted();
        self.runtime.kill_best_effort();
        drop(self.reader.take());
        let runtime = self.runtime.clone();
        let spawn_permit = self.spawn_permit.take();
        // A spawn_blocking result is dropped when its awaiting HTTP request is
        // canceled. Reap on a detached native thread so that cancellation can
        // never leak the just-spawned shell or leave a zombie.
        thread::spawn(move || {
            let _spawn_permit = spawn_permit;
            let result = child.wait();
            match result {
                Ok(status) => runtime.mark_reaped(
                    TerminalPaneStatus::Interrupted,
                    Some(status.exit_code()),
                    true,
                ),
                Err(error) => {
                    tracing::debug!(%error, runtime_id = %runtime.id, "failed to reap uncommitted terminal runtime");
                    runtime.mark_reaped(TerminalPaneStatus::Interrupted, None, false);
                }
            }
        });
    }
}

pub(super) fn spawn_runtime_blocking(
    id: String,
    spec: TerminalRuntimeSpec,
    spawn_permit: SpawnPermit,
    vt_worker: Option<VtWorkerClient>,
) -> Result<SpawnedRuntime, TerminaldError> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| TerminaldError::Pty(error.to_string()))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| TerminaldError::Pty(error.to_string()))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| TerminaldError::Pty(error.to_string()))?;
    let mut command = CommandBuilder::new(&spec.shell);
    command.args(&spec.arguments);
    for (key, value) in &spec.environment {
        command.env(key, value);
    }
    command.cwd(&spec.cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "aow-web");
    command.env("AOW_WEB_TERMINAL", "1");
    // This PTY is a remote terminal from the user's point of view. Programs
    // such as TraeCode CLI must use terminal-mediated clipboard protocols (OSC 52)
    // instead of writing to terminald's host clipboard. Keep the standard
    // four-field shape for applications that parse SSH_CONNECTION.
    command.env("SSH_CONNECTION", "127.0.0.1 0 127.0.0.1 0");
    command.env_remove("SSH_TTY");
    command.env_remove("TMUX");
    command.env_remove("TMUX_PANE");
    #[cfg(target_os = "macos")]
    command.env_remove("AOW_LOG_MODE");
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| TerminaldError::Pty(error.to_string()))?;
    let child_pid = child.process_id();
    let child_session_id = child_pid
        .and_then(|pid| i32::try_from(pid).ok())
        .filter(|pid| unsafe { libc::getsid(*pid) == *pid });
    let killer = child.clone_killer();
    drop(pair.slave);
    let (events, _) = broadcast::channel(OUTPUT_CHANNEL_CAPACITY);
    let (controller_changed, _) = watch::channel(None);
    let stream_epoch = Uuid::new_v4().to_string();
    let vt_session = vt_worker
        .as_ref()
        .map(|worker| worker.create_session(stream_epoch.clone(), spec.cols, spec.rows));
    Ok(SpawnedRuntime {
        runtime: Arc::new(Runtime {
            id,
            stream_epoch,
            creation_spec: spec.clone(),
            metadata: Mutex::new(RuntimeMetadata {
                rows: spec.rows,
                cols: spec.cols,
                status: TerminalPaneStatus::Running,
                exit_code: None,
            }),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            child_pid,
            child_session_id,
            #[cfg(target_os = "macos")]
            child_start_time: child_session_id
                .and_then(|pid| aow_process::info(pid).ok())
                .map(|info| info.start_time),
            deleted: AtomicBool::new(false),
            output: Mutex::new(OutputState {
                scrollback: VecDeque::new(),
                base_offset: 0,
                next_offset: 0,
                closed: false,
                state_parser: vte::Parser::new(),
                terminal: TerminalState::default(),
            }),
            vt_session,
            events,
            controller: Mutex::new(ControllerState {
                generation: 0,
                owner: None,
            }),
            controller_changed,
            delivery_gate: AsyncMutex::new(()),
            deleted_changed: watch::channel(false).0,
            reap_state: Mutex::new(false),
            reaped: Condvar::new(),
        }),
        reader: Some(reader),
        child: Some(child),
        spawn_permit: Some(spawn_permit),
    })
}

#[cfg(target_os = "macos")]
pub(super) fn macos_kill_session_members(session_id: i32, start_time: Option<&str>) {
    let Some(start_time) = start_time.filter(|_| session_id > 1) else {
        return;
    };
    // A completed session can disappear before this cleanup runs. Never act
    // on a new session that happens to reuse the old leader's PID.
    match aow_process::info(session_id) {
        Ok(leader) if leader.start_time != start_time => return,
        Err(error) if !matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) => return,
        _ => {}
    }
    let pids = match aow_process::list_pids() {
        Ok(pids) => pids,
        Err(error) => {
            tracing::warn!(%error, session_id, "failed to enumerate terminal session for cleanup");
            return;
        }
    };
    let members: Vec<_> = pids
        .into_iter()
        .filter_map(|pid| aow_process::info(pid).ok())
        .filter(|info| info.pid > 1 && info.session == session_id)
        .collect();
    for member in members {
        // Revalidate both identity and membership immediately before signaling.
        if aow_process::info(member.pid).is_ok_and(|current| {
            current.start_time == member.start_time && current.session == session_id
        }) {
            unsafe {
                libc::kill(member.pid, libc::SIGKILL);
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn linux_session_members(session_id: i32) -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                return false;
            };
            stat.rsplit_once(')')
                .and_then(|(_, fields)| fields.split_whitespace().nth(3))
                .and_then(|session| session.parse::<i32>().ok())
                == Some(session_id)
        })
        .collect()
}
