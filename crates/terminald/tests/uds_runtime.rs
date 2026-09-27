use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use aow_protocol::{
    TerminalAttachClientMessage, TerminalAttachServerMessage, TerminalControlState,
    TerminalPaneStatus, TerminalRuntimeSpec,
};
use aow_terminald::{VtWorkerConfig, run_with_shutdown, run_with_shutdown_and_vt_worker};
use aow_terminald_client::{TerminaldAttachStream, TerminaldClient, TerminaldClientError};
use futures_util::{SinkExt, StreamExt};
use tempfile::TempDir;
use tokio::{process::Command, sync::oneshot, task::JoinHandle, time::timeout};
use tokio_tungstenite::tungstenite::Message;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(target_os = "macos")]
#[path = "support/macos_cleanup.rs"]
mod macos_cleanup;

struct TestDaemon {
    client: TerminaldClient,
    socket: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), aow_terminald::TerminaldError>>>,
    _directory: TempDir,
}

impl TestDaemon {
    async fn start() -> Self {
        Self::start_inner(false).await
    }

    async fn start_with_stale_socket() -> Self {
        Self::start_inner(true).await
    }

    async fn start_with_vt_worker(config: VtWorkerConfig) -> Self {
        Self::start_inner_with_worker(false, Some(config)).await
    }

    async fn start_inner(create_stale_socket: bool) -> Self {
        Self::start_inner_with_worker(create_stale_socket, None).await
    }

    async fn start_inner_with_worker(
        create_stale_socket: bool,
        worker: Option<VtWorkerConfig>,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("terminald/terminald.sock");
        if create_stale_socket {
            std::fs::create_dir(socket.parent().unwrap()).unwrap();
            std::fs::set_permissions(
                socket.parent().unwrap(),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
            drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
            assert!(socket.exists());
        }
        let (shutdown, receiver) = oneshot::channel();
        let server_socket = socket.clone();
        let task = tokio::spawn(async move {
            match worker {
                Some(worker) => {
                    run_with_shutdown_and_vt_worker(
                        server_socket,
                        async move {
                            let _ = receiver.await;
                        },
                        Some(worker),
                    )
                    .await
                }
                None => {
                    run_with_shutdown(server_socket, async move {
                        let _ = receiver.await;
                    })
                    .await
                }
            }
        });
        let client = TerminaldClient::new(socket.clone());
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            if client.health().await.is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "terminald did not become ready at {}",
                socket.display()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Self {
            client,
            socket,
            shutdown: Some(shutdown),
            task: Some(task),
            _directory: directory,
        }
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let result = timeout(IO_TIMEOUT, self.task.take().unwrap())
            .await
            .expect("terminald shutdown timed out")
            .expect("terminald task panicked");
        result.expect("terminald shutdown failed");
        assert!(!self.socket.exists(), "socket was not cleaned up");
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn shell_spec(cwd: &std::path::Path) -> TerminalRuntimeSpec {
    TerminalRuntimeSpec {
        cwd: cwd.to_string_lossy().into_owned(),
        shell: "/bin/sh".to_owned(),
        arguments: Vec::new(),
        // Interactive /bin/sh prompts differ between macOS and Linux. They
        // must not be mistaken for protocol replies in control-only assertions.
        environment: [
            ("PS1".into(), "".into()),
            ("PS2".into(), "".into()),
            ("ENV".into(), "/dev/null".into()),
        ]
        .into(),
        rows: 24,
        cols: 80,
    }
}

async fn send_input(socket: &mut TerminaldAttachStream, input: &str) {
    socket
        .send(Message::Binary(input.as_bytes().to_vec().into()))
        .await
        .unwrap();
}

async fn prepare_shell(socket: &mut TerminaldAttachStream) {
    // Split the marker so terminal input echo cannot satisfy this barrier.
    // A fixed sleep does not prove the shell has executed stty on a busy host.
    send_input(socket, "stty -echo; printf '\\n__%s__\\n' 'SHELL_READY'\n").await;
    output_until(socket, b"__SHELL_READY__").await;
}

#[derive(Debug)]
struct StreamHandshake {
    epoch: String,
    offset: u64,
    reset: bool,
    replay_bytes: u64,
    restore: Option<String>,
    restore_cols: Option<u16>,
    restore_rows: Option<u16>,
    replay: Vec<u8>,
}

async fn stream_header(
    socket: &mut TerminaldAttachStream,
) -> (
    String,
    u64,
    bool,
    u64,
    Option<String>,
    Option<u16>,
    Option<u16>,
) {
    timeout(IO_TIMEOUT, async {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                let message: TerminalAttachServerMessage = serde_json::from_str(&text).unwrap();
                match message {
                    TerminalAttachServerMessage::Stream {
                        epoch,
                        offset,
                        reset,
                        replay_bytes,
                        restore,
                        restore_cols,
                        restore_rows,
                    } => (
                        epoch,
                        offset,
                        reset,
                        replay_bytes,
                        restore,
                        restore_cols,
                        restore_rows,
                    ),
                    other => panic!("first terminal server message was not stream: {other:?}"),
                }
            }
            Some(Ok(other)) => {
                panic!("first terminal server frame was not text: {other:?}")
            }
            Some(Err(error)) => panic!("WebSocket read failed: {error}"),
            None => panic!("WebSocket ended before stream handshake"),
        }
    })
    .await
    .expect("timed out waiting for terminal stream handshake")
}

async fn stream_handshake(socket: &mut TerminaldAttachStream) -> StreamHandshake {
    let (epoch, offset, reset, replay_bytes, restore, restore_cols, restore_rows) =
        stream_header(socket).await;
    assert_eq!(
        restore_cols.is_some(),
        restore_rows.is_some(),
        "snapshot restore dimensions must be present as a pair"
    );
    if restore_cols.is_some() {
        assert!(reset, "snapshot restore requires a reset stream");
        assert!(
            restore.is_some(),
            "snapshot dimensions require restore data"
        );
    }
    let replay_size = usize::try_from(replay_bytes).expect("test replay size fits in usize");
    let replay = timeout(IO_TIMEOUT, async {
        let mut replay = Vec::with_capacity(replay_size);
        while replay.len() < replay_size {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => replay.extend_from_slice(&bytes),
                Some(Ok(other)) => {
                    panic!("received {other:?} before declared replay completed")
                }
                Some(Err(error)) => panic!("WebSocket read failed: {error}"),
                None => panic!("WebSocket ended before declared replay completed"),
            }
        }
        assert_eq!(replay.len(), replay_size, "replay exceeded declared size");
        replay
    })
    .await
    .expect("timed out waiting for terminal replay");
    let handshake = StreamHandshake {
        epoch,
        offset,
        reset,
        replay_bytes,
        restore,
        restore_cols,
        restore_rows,
        replay,
    };
    debug_assert_eq!(
        handshake.restore_cols.is_some(),
        handshake.restore_rows.is_some()
    );
    handshake
}

async fn send_claim(socket: &mut TerminaldAttachStream, force: bool) {
    let claim = serde_json::to_string(&TerminalAttachClientMessage::Claim { force }).unwrap();
    socket.send(Message::Text(claim.into())).await.unwrap();
}

async fn wait_for_control(socket: &mut TerminaldAttachStream, expected: TerminalControlState) {
    loop {
        if next_control(socket).await == expected {
            return;
        }
    }
}

async fn next_control(socket: &mut TerminaldAttachStream) -> TerminalControlState {
    timeout(IO_TIMEOUT, async {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                let message: TerminalAttachServerMessage = serde_json::from_str(&text).unwrap();
                match message {
                    TerminalAttachServerMessage::Control { state } => state,
                    other => panic!("unexpected message before control state: {other:?}"),
                }
            }
            Some(Ok(other)) => panic!("unexpected frame before control state: {other:?}"),
            Some(Err(error)) => panic!("WebSocket read failed: {error}"),
            None => panic!("WebSocket ended before control state"),
        }
    })
    .await
    .expect("timed out waiting for terminal control state")
}

async fn controlled_claim(socket: &mut TerminaldAttachStream, force: bool) -> StreamHandshake {
    send_claim(socket, force).await;
    wait_for_control(socket, TerminalControlState::Claimed).await;
    stream_handshake(socket).await
}

async fn output_until(socket: &mut TerminaldAttachStream, marker: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let result = timeout(IO_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => {
                    output.extend_from_slice(&bytes);
                    if output.windows(marker.len()).any(|window| window == marker) {
                        return;
                    }
                }
                Some(Ok(Message::Text(_))) => {}
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(frame))) => {
                    panic!("WebSocket closed before output marker: {frame:?}")
                }
                Some(Err(error)) => panic!("WebSocket read failed: {error}"),
                None => panic!("WebSocket ended before output marker"),
                Some(Ok(Message::Frame(_))) => {}
            }
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "timed out waiting for {:?}; received {:?}",
        String::from_utf8_lossy(marker),
        String::from_utf8_lossy(&output)
    );
    output
}

async fn output_count_until(socket: &mut TerminaldAttachStream, marker: &[u8]) -> usize {
    timeout(IO_TIMEOUT, async {
        let mut total = 0;
        let mut tail = Vec::new();
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => {
                    total += bytes.len();
                    let mut searchable = Vec::with_capacity(tail.len() + bytes.len());
                    searchable.extend_from_slice(&tail);
                    searchable.extend_from_slice(&bytes);
                    if searchable
                        .windows(marker.len())
                        .any(|window| window == marker)
                    {
                        return total;
                    }
                    let retained = marker.len().saturating_sub(1).min(searchable.len());
                    tail.clear();
                    tail.extend_from_slice(&searchable[searchable.len() - retained..]);
                }
                Some(Ok(Message::Text(text))) => {
                    let message: TerminalAttachServerMessage = serde_json::from_str(&text).unwrap();
                    if let TerminalAttachServerMessage::Error { code, message } = message {
                        panic!("terminal error before output marker: {code}: {message}");
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(frame))) => {
                    panic!("WebSocket closed before output marker: {frame:?}")
                }
                Some(Err(error)) => panic!("WebSocket read failed: {error}"),
                None => panic!("WebSocket ended before output marker"),
                Some(Ok(Message::Frame(_))) => {}
            }
        }
    })
    .await
    .expect("timed out waiting for terminal output")
}

async fn drain_output_until_quiet(socket: &mut TerminaldAttachStream, quiet: Duration) -> Vec<u8> {
    let mut output = Vec::new();
    loop {
        match timeout(quiet, socket.next()).await {
            Err(_) => return output,
            Ok(Some(Ok(Message::Binary(bytes)))) => output.extend_from_slice(&bytes),
            Ok(Some(Ok(Message::Text(text)))) => {
                let message: TerminalAttachServerMessage = serde_json::from_str(&text).unwrap();
                if let TerminalAttachServerMessage::Error { code, message } = message {
                    panic!("terminal error while draining output: {code}: {message}");
                }
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)))) => {}
            Ok(Some(Ok(Message::Close(frame)))) => {
                panic!("WebSocket closed while draining output: {frame:?}")
            }
            Ok(Some(Err(error))) => panic!("WebSocket read failed: {error}"),
            Ok(None) => panic!("WebSocket ended while draining output"),
        }
    }
}

async fn wait_for_error(socket: &mut TerminaldAttachStream, expected: &str) {
    timeout(IO_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let message: TerminalAttachServerMessage = serde_json::from_str(&text).unwrap();
                    if let TerminalAttachServerMessage::Error { code, .. } = message
                        && code == expected
                    {
                        return;
                    }
                }
                Some(Ok(Message::Binary(_)))
                | Some(Ok(Message::Ping(_) | Message::Pong(_)))
                | Some(Ok(Message::Frame(_))) => {}
                Some(Ok(Message::Close(frame))) => {
                    panic!("WebSocket closed before {expected} error: {frame:?}")
                }
                Some(Err(error)) => panic!("WebSocket read failed: {error}"),
                None => panic!("WebSocket ended before {expected} error"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {expected} error"));
}

async fn wait_for_status(
    socket: &mut TerminaldAttachStream,
    expected: TerminalPaneStatus,
) -> Option<u32> {
    timeout(IO_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let message: TerminalAttachServerMessage = serde_json::from_str(&text).unwrap();
                    if let TerminalAttachServerMessage::Status { status, exit_code } = message
                        && status == expected
                    {
                        return exit_code;
                    }
                }
                Some(Ok(Message::Binary(_)))
                | Some(Ok(Message::Ping(_) | Message::Pong(_)))
                | Some(Ok(Message::Frame(_))) => {}
                Some(Ok(Message::Close(frame))) => {
                    panic!("WebSocket closed before status: {frame:?}")
                }
                Some(Err(error)) => panic!("WebSocket read failed: {error}"),
                None => panic!("WebSocket ended before status"),
            }
        }
    })
    .await
    .expect("timed out waiting for terminal status")
}

async fn wait_until_process_gone(pid: i32) {
    let deadline = Instant::now() + IO_TIMEOUT;
    loop {
        let result = unsafe { libc::kill(pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "process {pid} survived terminal runtime deletion"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn pid_between(output: &str, prefix: &str, suffix: &str) -> i32 {
    output
        .rsplit_once(prefix)
        .and_then(|(_, tail)| tail.split_once(suffix))
        .map(|(pid, _)| pid.trim_matches(|character: char| !character.is_ascii_digit()))
        .and_then(|pid| pid.parse::<i32>().ok())
        .unwrap_or_else(|| {
            panic!("could not parse PID between {prefix:?} and {suffix:?} from {output:?}")
        })
}

#[tokio::test]
async fn uds_rest_is_idempotent_and_socket_is_private() {
    let daemon = TestDaemon::start().await;
    let health = daemon.client.health().await.unwrap();
    assert_eq!(health.service, "aow-terminald");
    assert!(!health.instance_id.is_empty());

    let parent_mode = std::fs::metadata(daemon.socket.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let socket_mode = std::fs::metadata(&daemon.socket)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(parent_mode, 0o700);
    assert_eq!(socket_mode, 0o600);

    let cwd = tempfile::tempdir().unwrap();
    let spec = shell_spec(cwd.path());
    let mut invalid_cwd = spec.clone();
    invalid_cwd.cwd = "relative/path".to_owned();
    assert!(matches!(
        daemon.client.create("invalid-cwd", &invalid_cwd).await,
        Err(TerminaldClientError::HttpStatus {
            status: axum::http::StatusCode::BAD_REQUEST,
            ..
        })
    ));
    let mut missing_shell = spec.clone();
    missing_shell.shell = "/definitely/missing/terminald-test-shell".to_owned();
    assert!(matches!(
        daemon.client.create("missing-shell", &missing_shell).await,
        Err(TerminaldClientError::HttpStatus {
            status: axum::http::StatusCode::BAD_REQUEST,
            ..
        })
    ));

    let created = daemon.client.create("stable-id", &spec).await.unwrap();
    assert_eq!(created.id, "stable-id");
    assert_eq!(created.status, TerminalPaneStatus::Running);
    let mut reported_spec = spec.clone();
    reported_spec.environment.clear(); // Launch-only environment is never returned by HTTP.
    assert_eq!(created.spec(), reported_spec);

    let existing = daemon.client.create("stable-id", &spec).await.unwrap();
    assert_eq!(existing, created);
    assert_eq!(daemon.client.get("stable-id").await.unwrap(), Some(created));
    assert_eq!(daemon.client.list().await.unwrap().len(), 1);

    let reserved_id = "reserved:/?#[]@!$&'()*+,;=%é";
    let reserved = daemon.client.create(reserved_id, &spec).await.unwrap();
    assert_eq!(reserved.id, reserved_id);
    assert_eq!(
        daemon.client.get(reserved_id).await.unwrap(),
        Some(reserved)
    );

    let concurrent_left = daemon.client.create("concurrent-id", &spec);
    let concurrent_right = daemon.client.create("concurrent-id", &spec);
    let (concurrent_left, concurrent_right) = tokio::join!(concurrent_left, concurrent_right);
    assert_eq!(concurrent_left.unwrap(), concurrent_right.unwrap());
    assert_eq!(daemon.client.list().await.unwrap().len(), 3);

    let mut conflicting = spec.clone();
    conflicting.cols += 1;
    let error = daemon
        .client
        .create("stable-id", &conflicting)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        TerminaldClientError::HttpStatus {
            status: axum::http::StatusCode::CONFLICT,
            ..
        }
    ));

    daemon.client.delete("stable-id").await.unwrap();
    daemon.client.delete("concurrent-id").await.unwrap();
    daemon.client.delete(reserved_id).await.unwrap();
    daemon.client.delete("stable-id").await.unwrap();
    assert!(daemon.client.get("stable-id").await.unwrap().is_none());
    let attach_error = daemon.client.attach("stable-id").await.unwrap_err();
    assert!(matches!(
        attach_error,
        TerminaldClientError::WebSocket(tokio_tungstenite::tungstenite::Error::Http(response))
            if response.status() == axum::http::StatusCode::NOT_FOUND
    ));
    assert!(daemon.client.list().await.unwrap().is_empty());
    daemon.stop().await;
}

#[tokio::test]
async fn same_uid_stale_socket_is_replaced_and_instances_are_ephemeral() {
    let first = TestDaemon::start_with_stale_socket().await;
    let first_instance = first.client.health().await.unwrap().instance_id;
    first.stop().await;

    let second = TestDaemon::start().await;
    let second_instance = second.client.health().await.unwrap().instance_id;
    assert_ne!(first_instance, second_instance);
    second.stop().await;
}

#[tokio::test]
async fn live_socket_is_never_replaced() {
    let daemon = TestDaemon::start().await;
    let error = run_with_shutdown(daemon.socket.clone(), std::future::pending())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        aow_terminald::TerminaldError::SocketInUse(path) if path == daemon.socket
    ));
    assert_eq!(
        daemon.client.health().await.unwrap().service,
        "aow-terminald"
    );
    daemon.stop().await;
}

#[tokio::test]
async fn controlled_attach_is_neutral_and_claim_wait_force_and_release_are_exclusive() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("controlled", &shell_spec(cwd.path()))
        .await
        .unwrap();

    let mut first = daemon
        .client
        .attach_controlled_with_resume("controlled", None, None)
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_millis(100), first.next())
            .await
            .is_err(),
        "controlled attach must be neutral before claim"
    );
    send_input(&mut first, "echo __NEUTRAL_INPUT__\n").await;
    wait_for_error(&mut first, "controller_required").await;
    let resize =
        serde_json::to_string(&TerminalAttachClientMessage::Resize { cols: 99, rows: 39 }).unwrap();
    first.send(Message::Text(resize.into())).await.unwrap();
    wait_for_error(&mut first, "controller_required").await;

    let first_stream = controlled_claim(&mut first, false).await;
    assert!(first_stream.reset);
    assert!(first_stream.restore.is_none());
    wait_for_status(&mut first, TerminalPaneStatus::Running).await;

    let mut waiting = daemon
        .client
        .attach_controlled_with_resume("controlled", None, None)
        .await
        .unwrap();
    send_claim(&mut waiting, false).await;
    wait_for_control(&mut waiting, TerminalControlState::Waiting).await;
    assert!(
        timeout(Duration::from_millis(100), waiting.next())
            .await
            .is_err(),
        "waiting attachment must not receive output or status"
    );
    send_input(&mut waiting, "echo __WAITING_INPUT__\n").await;
    wait_for_error(&mut waiting, "controller_required").await;
    send_input(&mut first, "printf '__AFTER_WAITING_CLAIM__\n'\n").await;
    output_until(&mut first, b"__AFTER_WAITING_CLAIM__").await;

    send_claim(&mut waiting, true).await;
    wait_for_control(&mut waiting, TerminalControlState::Claimed).await;
    let forced_stream = stream_handshake(&mut waiting).await;
    assert!(forced_stream.reset);
    assert!(
        forced_stream
            .replay
            .windows(b"__AFTER_WAITING_CLAIM__".len())
            .any(|window| window == b"__AFTER_WAITING_CLAIM__"),
        "resume snapshot must be computed when claim succeeds"
    );
    assert!(
        !forced_stream
            .replay
            .windows(b"__WAITING_INPUT__".len())
            .any(|window| window == b"__WAITING_INPUT__"),
        "waiting input must not reach the PTY"
    );
    wait_for_status(&mut waiting, TerminalPaneStatus::Running).await;
    wait_for_error(&mut first, "attachment_superseded").await;

    // `first` exits after supersede and performs a delayed conditional
    // release. The new generation must remain owner after that stale cleanup.
    drop(first);
    tokio::time::sleep(Duration::from_millis(50)).await;
    send_input(&mut waiting, "stty -echo; printf '__FORCED_OWNER__\n'\n").await;
    output_until(&mut waiting, b"__FORCED_OWNER__").await;
    drop(waiting);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut released = daemon
        .client
        .attach_controlled_with_resume("controlled", None, None)
        .await
        .unwrap();
    controlled_claim(&mut released, false).await;
    wait_for_status(&mut released, TerminalPaneStatus::Running).await;
    send_input(&mut released, "printf '__RELEASED_OWNER__\n'\n").await;
    output_until(&mut released, b"__RELEASED_OWNER__").await;

    daemon.client.delete("controlled").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn observer_receives_output_without_mutation_until_it_forces_takeover() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("observer", &shell_spec(cwd.path()))
        .await
        .unwrap();

    let mut controller = daemon
        .client
        .attach_controlled_with_resume("observer", None, None)
        .await
        .unwrap();
    controlled_claim(&mut controller, false).await;
    wait_for_status(&mut controller, TerminalPaneStatus::Running).await;
    send_input(&mut controller, "printf '__OBSERVER_REPLAY__\n'\n").await;
    output_until(&mut controller, b"__OBSERVER_REPLAY__").await;

    let mut observer = daemon
        .client
        .attach_controlled_with_resume_capabilities_and_observer(
            "observer", None, None, false, true,
        )
        .await
        .unwrap();
    send_claim(&mut observer, false).await;
    wait_for_control(&mut observer, TerminalControlState::Observing).await;
    let observer_stream = stream_handshake(&mut observer).await;
    assert!(
        observer_stream
            .replay
            .windows(b"__OBSERVER_REPLAY__".len())
            .any(|window| window == b"__OBSERVER_REPLAY__"),
        "observer must receive the existing terminal replay"
    );
    wait_for_status(&mut observer, TerminalPaneStatus::Running).await;

    send_input(&mut observer, "printf '__OBSERVER_INPUT__\n'\n").await;
    wait_for_error(&mut observer, "controller_required").await;
    let resize = serde_json::to_string(&TerminalAttachClientMessage::Resize {
        cols: 111,
        rows: 43,
    })
    .unwrap();
    observer.send(Message::Text(resize.into())).await.unwrap();
    wait_for_error(&mut observer, "controller_required").await;

    send_input(&mut controller, "printf '__OBSERVER_LIVE__\n'\n").await;
    let observed_output = output_until(&mut observer, b"__OBSERVER_LIVE__").await;
    assert!(
        !observed_output
            .windows(b"__OBSERVER_INPUT__".len())
            .any(|window| window == b"__OBSERVER_INPUT__"),
        "read-only input must never reach the PTY"
    );
    // The live marker can be followed by the shell prompt in a separate
    // broadcast frame. Drain that already-in-flight observer output so the
    // next frame observed here is the explicit takeover acknowledgement.
    let trailing_output = drain_output_until_quiet(&mut observer, Duration::from_millis(100)).await;
    assert!(
        !trailing_output
            .windows(b"__OBSERVER_INPUT__".len())
            .any(|window| window == b"__OBSERVER_INPUT__"),
        "read-only input must never reach the PTY"
    );

    send_claim(&mut observer, true).await;
    wait_for_control(&mut observer, TerminalControlState::Claimed).await;
    stream_handshake(&mut observer).await;
    wait_for_status(&mut observer, TerminalPaneStatus::Running).await;
    wait_for_error(&mut controller, "attachment_superseded").await;
    send_input(&mut observer, "printf '__OBSERVER_TAKEOVER__\n'\n").await;
    output_until(&mut observer, b"__OBSERVER_TAKEOVER__").await;

    let runtime = daemon.client.get("observer").await.unwrap().unwrap();
    assert_eq!(
        (runtime.rows, runtime.cols),
        (24, 80),
        "observer resize must not alter PTY dimensions"
    );
    drop(observer);
    drop(controller);
    daemon.client.delete("observer").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn waiting_attachment_auto_claims_vacancy_and_repeated_claim_is_idempotent() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("auto-claim", &shell_spec(cwd.path()))
        .await
        .unwrap();

    let mut owner = daemon
        .client
        .attach_controlled_with_resume("auto-claim", None, None)
        .await
        .unwrap();
    controlled_claim(&mut owner, false).await;
    wait_for_status(&mut owner, TerminalPaneStatus::Running).await;

    let mut first_waiter = daemon
        .client
        .attach_controlled_with_resume("auto-claim", None, None)
        .await
        .unwrap();
    let mut second_waiter = daemon
        .client
        .attach_controlled_with_resume("auto-claim", None, None)
        .await
        .unwrap();
    send_claim(&mut first_waiter, false).await;
    send_claim(&mut second_waiter, false).await;
    wait_for_control(&mut first_waiter, TerminalControlState::Waiting).await;
    wait_for_control(&mut second_waiter, TerminalControlState::Waiting).await;
    drop(owner);

    let first_state = next_control(&mut first_waiter).await;
    let second_state = next_control(&mut second_waiter).await;
    assert!(
        matches!(
            (first_state, second_state),
            (TerminalControlState::Claimed, TerminalControlState::Waiting)
                | (TerminalControlState::Waiting, TerminalControlState::Claimed)
        ),
        "exactly one waiter must win the vacant controller"
    );
    let (waiter, still_waiting) = if first_state == TerminalControlState::Claimed {
        (&mut first_waiter, &mut second_waiter)
    } else {
        (&mut second_waiter, &mut first_waiter)
    };
    stream_handshake(waiter).await;
    wait_for_status(waiter, TerminalPaneStatus::Running).await;
    assert!(
        timeout(Duration::from_millis(100), still_waiting.next())
            .await
            .is_err(),
        "losing waiter must remain connected without output/status"
    );

    for force in [false, true] {
        send_claim(waiter, force).await;
        // A write acknowledgement is ordered after the claim on this socket.
        // Normal PTY output (including macOS shell initialization escapes) can
        // still arrive, but a repeated claim must not emit another handshake.
        let request_id = format!("idempotent-owner-{force}");
        let write = serde_json::to_string(&TerminalAttachClientMessage::Write {
            request_id: request_id.clone(),
            // Split the marker so terminal input echo cannot satisfy the check.
            data: "printf '\\n__%s__\\n' 'IDEMPOTENT_OWNER'\n".to_owned(),
        })
        .unwrap();
        waiter.send(Message::Text(write.into())).await.unwrap();
        timeout(IO_TIMEOUT, async {
            loop {
                match waiter.next().await {
                    Some(Ok(Message::Text(text))) => {
                        let message: TerminalAttachServerMessage =
                            serde_json::from_str(&text).unwrap();
                        match message {
                            TerminalAttachServerMessage::Written { request_id: actual } => {
                                assert_eq!(actual, request_id);
                                break;
                            }
                            other => panic!(
                                "repeated claim emitted an unexpected control message: {other:?}"
                            ),
                        }
                    }
                    Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => {}
                    other => panic!("WebSocket failed before write acknowledgement: {other:?}"),
                }
            }
        })
        .await
        .expect("repeated claim must preserve the owner's ability to write");
        output_until(waiter, b"__IDEMPOTENT_OWNER__").await;
    }

    daemon.client.delete("auto-claim").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn stale_superseded_disconnect_does_not_release_the_new_owner() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("stale-release", &shell_spec(cwd.path()))
        .await
        .unwrap();

    let mut stale = daemon
        .client
        .attach_controlled_with_resume("stale-release", None, None)
        .await
        .unwrap();
    controlled_claim(&mut stale, false).await;
    wait_for_status(&mut stale, TerminalPaneStatus::Running).await;

    let mut owner = daemon
        .client
        .attach_controlled_with_resume("stale-release", None, None)
        .await
        .unwrap();
    controlled_claim(&mut owner, true).await;
    wait_for_status(&mut owner, TerminalPaneStatus::Running).await;
    wait_for_error(&mut stale, "attachment_superseded").await;
    drop(stale);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut contender = daemon
        .client
        .attach_controlled_with_resume("stale-release", None, None)
        .await
        .unwrap();
    send_claim(&mut contender, false).await;
    wait_for_control(&mut contender, TerminalControlState::Waiting).await;
    send_input(&mut owner, "printf '__NEW_OWNER_SURVIVED__\n'\n").await;
    output_until(&mut owner, b"__NEW_OWNER_SURVIVED__").await;

    daemon.client.delete("stale-release").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn non_socket_at_socket_path_is_never_removed() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("terminald");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = parent.join("terminald.sock");
    std::fs::write(&socket, b"do-not-delete").unwrap();

    let error = run_with_shutdown(socket.clone(), std::future::pending())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        aow_terminald::TerminaldError::UnsafeSocket(_)
    ));
    assert_eq!(std::fs::read(socket).unwrap(), b"do-not-delete");
}

#[tokio::test]
async fn real_pty_attach_resize_and_detach_preserve_runtime() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("interactive", &shell_spec(cwd.path()))
        .await
        .unwrap();
    let mut first = daemon.client.attach("interactive").await.unwrap();
    assert_eq!(
        wait_for_status(&mut first, TerminalPaneStatus::Running).await,
        None
    );
    prepare_shell(&mut first).await;
    send_input(&mut first, "echo __FIRST_ATTACH__\n").await;
    output_until(&mut first, b"__FIRST_ATTACH__").await;

    let resize = serde_json::to_string(&TerminalAttachClientMessage::Resize {
        cols: 101,
        rows: 41,
    })
    .unwrap();
    first.send(Message::Text(resize.into())).await.unwrap();
    send_input(&mut first, "stty size; echo __RESIZED__\n").await;
    let resized_output = output_until(&mut first, b"__RESIZED__").await;
    assert!(
        String::from_utf8_lossy(&resized_output).contains("41 101"),
        "PTY did not report the resized dimensions: {}",
        String::from_utf8_lossy(&resized_output)
    );

    drop(first);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let runtime = daemon.client.get("interactive").await.unwrap().unwrap();
    assert_eq!(runtime.status, TerminalPaneStatus::Running);
    assert_eq!((runtime.rows, runtime.cols), (41, 101));
    let original_spec = shell_spec(cwd.path());
    let existing_after_resize = daemon
        .client
        .create("interactive", &original_spec)
        .await
        .unwrap();
    assert_eq!(
        (existing_after_resize.rows, existing_after_resize.cols),
        (41, 101)
    );

    let mut second = daemon.client.connect_attach("interactive").await.unwrap();
    let scrollback = output_until(&mut second, b"__FIRST_ATTACH__").await;
    assert!(
        scrollback
            .windows(11)
            .any(|window| window == b"__RESIZED__")
    );
    send_input(&mut second, "echo __SECOND_ATTACH__\n").await;
    output_until(&mut second, b"__SECOND_ATTACH__").await;
    drop(second);

    daemon.client.delete("interactive").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn web_runtime_exposes_terminal_environment() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    let spec = shell_spec(cwd.path());
    #[cfg(target_os = "macos")]
    let spec = {
        let mut spec = spec;
        spec.environment
            .insert("AOW_LOG_MODE".into(), "unified".into());
        spec
    };
    daemon.client.create("clipboard-env", &spec).await.unwrap();

    let mut socket = daemon.client.attach("clipboard-env").await.unwrap();
    stream_handshake(&mut socket).await;
    wait_for_status(&mut socket, TerminalPaneStatus::Running).await;
    prepare_shell(&mut socket).await;
    drain_output_until_quiet(&mut socket, Duration::from_millis(50)).await;
    send_input(
        &mut socket,
        "printf '__TERMINAL_ENV__%s|%s|%s|%s__END__\n' \"$AOW_WEB_TERMINAL\" \"$TERM_PROGRAM\" \"$SSH_CONNECTION:${SSH_TTY-}\" \"${TMUX-}:${TMUX_PANE-}\"\n",
    )
    .await;
    let output = output_until(&mut socket, b"__END__").await;
    let output = String::from_utf8_lossy(&output);
    assert!(
        output.contains("__TERMINAL_ENV__1|aow-web|127.0.0.1 0 127.0.0.1 0:|:__END__"),
        "unexpected web terminal environment: {output}"
    );
    #[cfg(target_os = "macos")]
    {
        send_input(
            &mut socket,
            "printf '__LOG_MODE__%s__END__\\n' \"${AOW_LOG_MODE-unset}\"\n",
        )
        .await;
        let output = output_until(&mut socket, b"__END__").await;
        assert!(String::from_utf8_lossy(&output).contains("__LOG_MODE__unset__END__"));
    }

    daemon.client.delete("clipboard-env").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn fresh_reattach_uses_vt_snapshot_and_contiguous_raw_delta() {
    let worker =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vt-worker/dist/vt-worker.mjs");
    if !worker.exists() {
        eprintln!("skipping VT reattach test because the worker bundle is unavailable");
        return;
    }
    let daemon = TestDaemon::start_with_vt_worker(VtWorkerConfig::new("node", worker)).await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("vt-reattach", &shell_spec(cwd.path()))
        .await
        .unwrap();

    let mut first = daemon
        .client
        .attach_with_resume_capabilities("vt-reattach", None, None, true)
        .await
        .unwrap();
    stream_handshake(&mut first).await;
    wait_for_status(&mut first, TerminalPaneStatus::Running).await;
    send_input(
        &mut first,
        "stty -echo; printf '\\033[?1049h\\033[2J\\033[H__VT_SNAPSHOT_MARKER__\\n'\n",
    )
    .await;
    output_until(&mut first, b"__VT_SNAPSHOT_MARKER__").await;
    drain_output_until_quiet(&mut first, Duration::from_millis(350)).await;
    drop(first);
    // This unique suffix is emitted after detach. Depending on exact actor
    // scheduling it is either already represented by the serialized state or
    // arrives in the raw delta, but the combined restoration must contain it
    // exactly once.
    let late_marker = b"__VT_LATE_DELTA_MARKER__";
    let mut writer = daemon
        .client
        .attach_with_resume_capabilities("vt-reattach", None, None, true)
        .await
        .unwrap();
    stream_handshake(&mut writer).await;
    wait_for_status(&mut writer, TerminalPaneStatus::Running).await;
    send_input(&mut writer, "printf '__VT_LATE_DELTA_MARKER__\n'\n").await;
    output_until(&mut writer, late_marker).await;
    drop(writer);

    // An old client does not know how to parse a serialized VT stream at the
    // snapshot's original geometry. It must keep receiving the pre-extension
    // raw reset even while terminald has a valid cached snapshot.
    let mut legacy = daemon.client.attach("vt-reattach").await.unwrap();
    let legacy_restore = stream_handshake(&mut legacy).await;
    assert_eq!(legacy_restore.restore, None);
    assert_eq!(legacy_restore.restore_cols, None);
    assert_eq!(legacy_restore.restore_rows, None);
    assert!(
        legacy_restore
            .replay
            .windows(late_marker.len())
            .any(|window| window == late_marker),
        "legacy client did not receive raw terminal history"
    );
    drop(legacy);

    let mut fresh = daemon
        .client
        .attach_with_resume_capabilities("vt-reattach", None, None, true)
        .await
        .unwrap();
    let restored = stream_handshake(&mut fresh).await;
    assert!(restored.reset);
    assert_eq!(
        (restored.restore_cols, restored.restore_rows),
        (Some(80), Some(24))
    );
    let restore = restored.restore.expect("fresh attach carries VT snapshot");
    assert!(
        restore.contains("__VT_SNAPSHOT_MARKER__"),
        "serialized VT state omitted marker: {restore:?}"
    );
    assert!(
        restore.contains("\u{1b}[?1049h"),
        "serialized VT state did not restore the fullscreen alternate screen: {restore:?}"
    );
    assert_eq!(restored.replay_bytes, restored.replay.len() as u64);
    let mut restored_bytes = restore.into_bytes();
    restored_bytes.extend_from_slice(&restored.replay);
    assert_eq!(
        restored_bytes
            .windows(late_marker.len())
            .filter(|window| *window == late_marker)
            .count(),
        1,
        "snapshot plus raw delta must contain detached output exactly once"
    );

    daemon.client.delete("vt-reattach").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn terminald_binary_uses_the_embedded_worker_by_default() {
    if Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_err()
    {
        eprintln!("skipping embedded worker test because Node.js is unavailable");
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("terminald/terminald.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_aow-terminald"))
        .arg("--socket")
        .arg(&socket)
        .arg("--vt-node")
        .arg("node")
        .env_remove("AOW_TERMINALD_VT_WORKER")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("terminald binary starts");
    let client = TerminaldClient::new(socket.clone());
    let deadline = Instant::now() + IO_TIMEOUT;
    loop {
        if client.health().await.is_ok() {
            break;
        }
        assert!(
            daemon.try_wait().unwrap().is_none(),
            "terminald exited early"
        );
        assert!(
            Instant::now() < deadline,
            "terminald binary did not become ready"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let cwd = tempfile::tempdir().unwrap();
    client
        .create("embedded-worker", &shell_spec(cwd.path()))
        .await
        .unwrap();
    let mut first = client
        .attach_with_resume_capabilities("embedded-worker", None, None, true)
        .await
        .unwrap();
    stream_handshake(&mut first).await;
    wait_for_status(&mut first, TerminalPaneStatus::Running).await;
    send_input(
        &mut first,
        "stty -echo; printf '__EMBEDDED_VT_MARKER__\n'\n",
    )
    .await;
    output_until(&mut first, b"__EMBEDDED_VT_MARKER__").await;
    drain_output_until_quiet(&mut first, Duration::from_millis(350)).await;
    drop(first);

    let mut fresh = client
        .attach_with_resume_capabilities("embedded-worker", None, None, true)
        .await
        .unwrap();
    let restored = stream_handshake(&mut fresh).await;
    assert_eq!(
        (restored.restore_cols, restored.restore_rows),
        (Some(80), Some(24))
    );
    assert!(
        restored
            .restore
            .as_deref()
            .is_some_and(|restore| restore.contains("__EMBEDDED_VT_MARKER__")),
        "fresh attach did not use the binary's embedded VT worker"
    );
    drop(fresh);
    client.delete("embedded-worker").await.unwrap();

    let pid = i32::try_from(daemon.id().expect("terminald child has a pid")).unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let status = timeout(IO_TIMEOUT, daemon.wait())
        .await
        .expect("terminald binary shutdown timed out")
        .expect("terminald binary wait failed");
    assert!(status.success(), "terminald exited with {status}");
    assert!(
        !socket.exists(),
        "terminald binary did not clean up its socket"
    );
}

#[tokio::test]
async fn stream_handshake_resumes_exactly_and_resets_stale_or_future_offsets() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("resume", &shell_spec(cwd.path()))
        .await
        .unwrap();

    let mut initial = daemon.client.attach("resume").await.unwrap();
    let initial_stream = stream_handshake(&mut initial).await;
    assert!(initial_stream.reset);
    assert_eq!(initial_stream.offset, 0);
    assert_eq!(
        initial_stream.replay_bytes as usize,
        initial_stream.replay.len()
    );
    wait_for_status(&mut initial, TerminalPaneStatus::Running).await;
    send_input(
        &mut initial,
        "stty -echo; printf '__PREFIX____SUFFIX__\\n'\n",
    )
    .await;
    let emitted = output_count_until(&mut initial, b"__SUFFIX__").await;
    let live_tail = drain_output_until_quiet(&mut initial, Duration::from_millis(100)).await;
    let after = initial_stream.offset
        + initial_stream.replay_bytes
        + u64::try_from(emitted + live_tail.len()).unwrap();
    drop(initial);

    let mut exact = daemon
        .client
        .attach_from("resume", &initial_stream.epoch, after)
        .await
        .unwrap();
    let exact_stream = stream_handshake(&mut exact).await;
    assert!(!exact_stream.reset);
    assert_eq!(exact_stream.offset, after);
    assert_eq!(exact_stream.replay_bytes, 0);
    assert!(exact_stream.replay.is_empty());
    wait_for_status(&mut exact, TerminalPaneStatus::Running).await;
    send_input(&mut exact, "printf '__EXACT_SUFFIX__\\n'\n").await;
    output_until(&mut exact, b"__EXACT_SUFFIX__").await;
    drop(exact);

    let mut reset = daemon
        .client
        .attach_from("resume", &initial_stream.epoch, u64::MAX)
        .await
        .unwrap();
    let reset_stream = stream_handshake(&mut reset).await;
    assert!(reset_stream.reset);
    assert_eq!(reset_stream.offset, 0);
    assert_eq!(
        reset_stream.replay_bytes as usize,
        reset_stream.replay.len()
    );
    assert!(
        reset_stream
            .replay
            .windows(10)
            .any(|window| window == b"__PREFIX__")
    );
    let suffix_start = reset_stream
        .replay
        .windows(10)
        .position(|window| window == b"__PREFIX__")
        .unwrap();
    let suffix_offset = reset_stream.offset + u64::try_from(suffix_start).unwrap();
    let expected_suffix = reset_stream.replay[suffix_start..].to_vec();
    let known_end = reset_stream.offset + reset_stream.replay_bytes;
    drop(reset);

    let mut suffix = daemon
        .client
        .attach_from("resume", &initial_stream.epoch, suffix_offset)
        .await
        .unwrap();
    let suffix_stream = stream_handshake(&mut suffix).await;
    assert!(!suffix_stream.reset);
    assert_eq!(suffix_stream.offset, suffix_offset);
    assert_eq!(suffix_stream.replay, expected_suffix);
    assert_eq!(
        suffix_stream.replay_bytes as usize,
        suffix_stream.replay.len()
    );
    drop(suffix);

    let mut future = daemon
        .client
        .attach_from("resume", &initial_stream.epoch, known_end + 1)
        .await
        .unwrap();
    let future_stream = stream_handshake(&mut future).await;
    assert!(future_stream.reset);
    assert_eq!(future_stream.offset, 0);
    assert_eq!(
        future_stream.replay_bytes as usize,
        future_stream.replay.len()
    );

    daemon.client.delete("resume").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn newest_attachment_supersedes_old_controller_and_blocks_old_mutations() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("controller", &shell_spec(cwd.path()))
        .await
        .unwrap();

    let mut old = daemon.client.attach("controller").await.unwrap();
    stream_handshake(&mut old).await;
    wait_for_status(&mut old, TerminalPaneStatus::Running).await;

    let mut newest = daemon.client.attach("controller").await.unwrap();
    stream_handshake(&mut newest).await;
    wait_for_status(&mut newest, TerminalPaneStatus::Running).await;

    // Queue both stale mutations before observing the superseded notification.
    // The daemon validates the generation around the actual PTY operation, so
    // neither can affect process input or runtime metadata.
    let _ = old
        .send(Message::Binary(
            b"printf '__STALE_INPUT__\\n'\n".to_vec().into(),
        ))
        .await;
    let stale_resize = serde_json::to_string(&TerminalAttachClientMessage::Resize {
        cols: 133,
        rows: 47,
    })
    .unwrap();
    let _ = old.send(Message::Text(stale_resize.into())).await;
    wait_for_error(&mut old, "attachment_superseded").await;

    let runtime = daemon.client.get("controller").await.unwrap().unwrap();
    assert_eq!((runtime.rows, runtime.cols), (24, 80));

    send_input(&mut newest, "stty -echo\n").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    drain_output_until_quiet(&mut newest, Duration::from_millis(50)).await;
    send_input(&mut newest, "stty size; printf '__SIZE_CHECK__\\n'\n").await;
    let size_output = output_until(&mut newest, b"__SIZE_CHECK__").await;
    assert!(
        String::from_utf8_lossy(&size_output).contains("24 80"),
        "stale resize changed the PTY: {}",
        String::from_utf8_lossy(&size_output)
    );
    assert!(
        !String::from_utf8_lossy(&size_output).contains("__STALE_INPUT__"),
        "stale input reached the PTY: {}",
        String::from_utf8_lossy(&size_output)
    );

    let resize = serde_json::to_string(&TerminalAttachClientMessage::Resize {
        cols: 111,
        rows: 43,
    })
    .unwrap();
    newest.send(Message::Text(resize.into())).await.unwrap();
    send_input(&mut newest, "stty size; printf '__NEW_RESIZE__\\n'\n").await;
    let resized = output_until(&mut newest, b"__NEW_RESIZE__").await;
    assert!(
        String::from_utf8_lossy(&resized).contains("43 111"),
        "new controller could not resize the PTY: {}",
        String::from_utf8_lossy(&resized)
    );

    daemon.client.delete("controller").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn resume_epoch_rejects_offsets_from_a_recreated_runtime() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    let spec = shell_spec(cwd.path());
    daemon.client.create("recreated", &spec).await.unwrap();

    let mut original = daemon.client.attach("recreated").await.unwrap();
    let original_stream = stream_handshake(&mut original).await;
    wait_for_status(&mut original, TerminalPaneStatus::Running).await;
    drop(original);
    daemon.client.delete("recreated").await.unwrap();

    daemon.client.create("recreated", &spec).await.unwrap();
    let mut recreated = daemon
        .client
        .attach_from("recreated", &original_stream.epoch, 0)
        .await
        .unwrap();
    let recreated_stream = stream_handshake(&mut recreated).await;
    assert_ne!(recreated_stream.epoch, original_stream.epoch);
    assert!(recreated_stream.reset);
    assert_eq!(recreated_stream.offset, 0);

    daemon.client.delete("recreated").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn natural_exit_is_retained_and_delete_kills_session_descendants() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();

    daemon
        .client
        .create("natural-exit", &shell_spec(cwd.path()))
        .await
        .unwrap();
    let mut natural = daemon.client.attach("natural-exit").await.unwrap();
    wait_for_status(&mut natural, TerminalPaneStatus::Running).await;
    send_input(&mut natural, "exit 7\n").await;
    assert_eq!(
        wait_for_status(&mut natural, TerminalPaneStatus::Exited).await,
        Some(7)
    );
    let retained = daemon.client.get("natural-exit").await.unwrap().unwrap();
    assert_eq!(retained.status, TerminalPaneStatus::Exited);
    assert_eq!(retained.exit_code, Some(7));

    daemon
        .client
        .create("natural-exit-child", &shell_spec(cwd.path()))
        .await
        .unwrap();
    let mut natural_child = daemon.client.attach("natural-exit-child").await.unwrap();
    wait_for_status(&mut natural_child, TerminalPaneStatus::Running).await;
    prepare_shell(&mut natural_child).await;
    send_input(
        &mut natural_child,
        "nohup sleep 300 >/dev/null 2>&1 & child_pid=$! ; echo __NATURAL_CHILD__${child_pid}__END_NATURAL_CHILD__; exit 0\n",
    )
    .await;
    let child_output = output_until(&mut natural_child, b"__END_NATURAL_CHILD__").await;
    let natural_child_pid = pid_between(
        &String::from_utf8_lossy(&child_output),
        "__NATURAL_CHILD__",
        "__END_NATURAL_CHILD__",
    );
    assert_eq!(
        wait_for_status(&mut natural_child, TerminalPaneStatus::Exited).await,
        Some(0)
    );
    wait_until_process_gone(natural_child_pid).await;

    daemon
        .client
        .create("force-delete", &shell_spec(cwd.path()))
        .await
        .unwrap();
    let mut forced = daemon.client.attach("force-delete").await.unwrap();
    wait_for_status(&mut forced, TerminalPaneStatus::Running).await;
    prepare_shell(&mut forced).await;
    send_input(
        &mut forced,
        "sleep 300 & child_pid=$! ; echo __SHELL_PID__$$__END_SHELL____CHILD_PID__${child_pid}__END_PID__\n",
    )
    .await;
    let output = output_until(&mut forced, b"__END_PID__").await;
    let output = String::from_utf8_lossy(&output);
    let shell_pid = pid_between(&output, "__SHELL_PID__", "__END_SHELL__");
    let child_pid = pid_between(&output, "__CHILD_PID__", "__END_PID__");
    assert_eq!(unsafe { libc::kill(shell_pid, 0) }, 0);
    assert_eq!(unsafe { libc::kill(child_pid, 0) }, 0);

    daemon.client.delete("force-delete").await.unwrap();
    wait_until_process_gone(shell_pid).await;
    wait_until_process_gone(child_pid).await;
    assert!(daemon.client.get("force-delete").await.unwrap().is_none());
    daemon.client.delete("natural-exit").await.unwrap();
    daemon.client.delete("natural-exit-child").await.unwrap();
    daemon.stop().await;
}

#[tokio::test]
async fn daemon_shutdown_kills_and_reaps_runtime_processes() {
    let daemon = TestDaemon::start().await;
    let cwd = tempfile::tempdir().unwrap();
    daemon
        .client
        .create("shutdown", &shell_spec(cwd.path()))
        .await
        .unwrap();
    let mut socket = daemon.client.attach("shutdown").await.unwrap();
    wait_for_status(&mut socket, TerminalPaneStatus::Running).await;
    prepare_shell(&mut socket).await;
    send_input(
        &mut socket,
        "sleep 300 & child_pid=$! ; echo __SHELL_PID__$$__END_SHELL____CHILD_PID__${child_pid}__END_PID__\n",
    )
    .await;
    let output = output_until(&mut socket, b"__END_PID__").await;
    let output = String::from_utf8_lossy(&output);
    let shell_pid = pid_between(&output, "__SHELL_PID__", "__END_SHELL__");
    let child_pid = pid_between(&output, "__CHILD_PID__", "__END_PID__");

    daemon.stop().await;
    wait_until_process_gone(shell_pid).await;
    wait_until_process_gone(child_pid).await;
}
