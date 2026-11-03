use crate::host::Host;
use anyhow::{Result, ensure};
use aow_acp::AcpService;
use aow_acpd_client::{
    Event, Health, RpcError,
    protocol::{Call, PROTOCOL_VERSION},
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
    routing::{get, post},
};
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use std::{
    future::Future,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
    sync::Arc,
};
use tokio::{
    sync::{broadcast, watch},
    task::JoinSet,
};

pub struct Options {
    pub state_dir: PathBuf,
    pub socket: PathBuf,
}
#[derive(Clone)]
struct Daemon {
    service: AcpService,
    host: Arc<Host>,
    instance_id: String,
    stopping: watch::Receiver<bool>,
}

/// Bind before opening the store. Exactly one daemon may own a state directory.
pub async fn run_with_shutdown(
    options: Options,
    shutdown: impl Future<Output = ()> + Send,
) -> Result<()> {
    ensure!(
        options.state_dir.is_absolute(),
        "acpd state directory must be absolute"
    );
    let (listener, _socket) = crate::socket::bind_socket(&options.socket).await?;
    tokio::fs::create_dir_all(options.state_dir.join("data/acp")).await?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(options.state_dir.join("data/acp/daemon.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "ACP state directory is already owned by another daemon"
    );
    let host = Arc::new(Host {
        root: std::fs::canonicalize(&options.state_dir)?,
        probe: false,
        filesystem: Default::default(),
        stopping: std::sync::atomic::AtomicBool::new(false),
        completed: broadcast::channel(512).0,
    });
    let service = AcpService::with_storage(
        host.clone(),
        Some(host.root.join("data/acp")),
        Some(host.root.join("cache/acp")),
    )?;
    let (stopping, stopped) = watch::channel(false);
    let state = Daemon {
        service: service.clone(),
        host: host.clone(),
        instance_id: uuid::Uuid::new_v4().to_string(),
        stopping: stopped,
    };
    let router = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/rpc", post(rpc))
        .route("/v1/events", get(events))
        .route("/v1/completions/ack", post(acknowledge))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(state);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            Some(result) = connections.join_next(), if !connections.is_empty() => { if let Err(error) = result { tracing::debug!(%error, "acpd connection ended"); } },
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let router = router.clone();
                let mut stopped = stopping.subscribe();
                connections.spawn(async move {
                    let connection = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), TowerToHyperService::new(router)).with_upgrades();
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {},
                        _ = stopped.changed() => { connection.as_mut().graceful_shutdown(); let _ = connection.await; }
                    }
                });
            }
        }
    }
    // Stop agent transports first so permission/login waits cannot delay shutdown forever.
    stopping.send_replace(true);
    host.stopping
        .store(true, std::sync::atomic::Ordering::Release);
    service.shutdown().await?;
    while connections.join_next().await.is_some() {}
    let _files = host.filesystem.write().await;
    service.flush_threads_on_quit()?;
    drop(lock);
    Ok(())
}
async fn health(State(state): State<Daemon>) -> Json<Health> {
    Json(Health {
        service: "aow-acpd".into(),
        protocol: PROTOCOL_VERSION,
        instance_id: state.instance_id,
        pid: std::process::id(),
    })
}
async fn rpc(
    State(state): State<Daemon>,
    Json(call): Json<Call>,
) -> Json<std::result::Result<serde_json::Value, RpcError>> {
    if call.protocol != PROTOCOL_VERSION {
        return Json(Err(RpcError {
            code: "unsupported_protocol".into(),
            message: format!("Unsupported acpd protocol {}", call.protocol),
        }));
    }
    if *state.stopping.borrow() {
        return Json(Err(RpcError {
            code: "unavailable".into(),
            message: "ACP daemon is shutting down".into(),
        }));
    }
    // Admitted operations outlive a lost web/CLI connection. Retrying must use the original ID.
    let outcome = aow_i18n::spawn(async move {
        let mut stopped = state.stopping.clone();
        if *stopped.borrow() {
            anyhow::bail!("ACP daemon is shutting down");
        }
        tokio::select! {
            result = aow_i18n::scope(call.language, crate::dispatch::dispatch(&state.service, state.host, call.request)) => result,
            _ = stopped.changed() => anyhow::bail!("ACP daemon is shutting down"),
        }
    })
    .await;
    let result = outcome
        .map_err(anyhow::Error::from)
        .and_then(|result| result)
        .map_err(|error| RpcError {
            code: if aow_acp::is_auth_required(&error) {
                "auth_required"
            } else if matches!(
                error.to_string().as_str(),
                "ACP session not found" | "ACP connection not found"
            ) {
                "not_found"
            } else {
                "invalid"
            }
            .into(),
            message: format!("{error:#}"),
        });
    Json(result)
}
async fn events(State(state): State<Daemon>, ws: WebSocketUpgrade) -> Response {
    let changes = state.service.subscribe_sync();
    let completed = state.host.completed.subscribe();
    ws.on_upgrade(move |socket| event_socket(socket, state, changes, completed))
}
async fn send(socket: &mut WebSocket, event: &Event) -> Result<()> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.send(Message::Text(serde_json::to_string(event)?.into())),
    )
    .await??;
    Ok(())
}
async fn event_socket(
    mut socket: WebSocket,
    mut state: Daemon,
    mut changes: broadcast::Receiver<aow_acp::SyncEvent>,
    mut completed: broadcast::Receiver<Event>,
) {
    let run: Result<()> = async {
        socket.send(Message::Text("ready".into())).await?;
        let mut pending = match tokio::fs::read_dir(state.host.outbox()).await {
            Ok(pending) => Some(pending),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if let Some(pending) = &mut pending {
            while let Some(entry) = pending.next_entry().await? {
                if entry.path().extension().is_some_and(|extension| extension == "json") {
                    match tokio::fs::read(entry.path()).await {
                        Ok(bytes) => send(&mut socket, &serde_json::from_slice::<Event>(&bytes)?).await?,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = state.stopping.changed() => break,
                change = changes.recv() => send(&mut socket, &Event::Sync(change?)).await?,
                completion = completed.recv() => send(&mut socket, &completion?).await?,
                _ = heartbeat.tick() => { tokio::time::timeout(std::time::Duration::from_secs(5), socket.send(Message::Ping(vec![].into()))).await??; },
                message = socket.recv() => match message {
                    Some(Ok(Message::Pong(_) | Message::Ping(_))) => {},
                    _ => break,
                }
            }
        }
        Ok(())
    }.await;
    if let Err(error) = run {
        tracing::debug!(%error, "acpd subscription closed; consumer must resync");
    }
    let _ = socket.send(Message::Close(None)).await;
}
#[derive(serde::Deserialize)]
struct Ack {
    id: String,
}
async fn acknowledge(
    State(state): State<Daemon>,
    Json(ack): Json<Ack>,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    uuid::Uuid::parse_str(&ack.id).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    match tokio::fs::remove_file(state.host.outbox().join(format!("{}.json", ack.id))).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
    }
    Ok(Json(serde_json::json!({})))
}
