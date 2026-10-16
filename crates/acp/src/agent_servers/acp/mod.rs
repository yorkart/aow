mod client;
mod debug_log;
mod transport;

use crate::{
    acp_thread::history::ThreadStore,
    facade::{AcpHost, ConnectionInfo},
    project::agent_server_store::AgentServerCommand,
};
use agent_client_protocol::schema::{ProtocolVersion, v1 as acp};
use agent_client_protocol::{Agent, Client, ConnectionTo, Lines, UntypedMessage};
use anyhow::{Context, Result, ensure};
use client::ClientContext;
pub(super) use debug_log::{AcpDebugLog, AcpDebugLogStore};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub(crate) struct AcpConnection {
    pub info: ConnectionInfo,
    connection: ConnectionTo<Agent>,
    pub context: Arc<ClientContext>,
    pub debug_log: AcpDebugLog,
    pub(super) loads: super::recovery::Loads,
    pub(super) load_identity: tokio::sync::Mutex<()>,
    stop: CancellationToken,
}

pub(super) struct StdioContext {
    pub events: crate::facade::events::SyncEvents,
    pub host: Arc<dyn AcpHost>,
    pub execution_path: Option<std::ffi::OsString>,
    pub threads: Arc<Mutex<ThreadStore>>,
    pub debug_log: AcpDebugLog,
}

impl Drop for AcpConnection {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl AcpConnection {
    pub(super) async fn stdio(
        agent_id: String,
        cwd: PathBuf,
        command: AgentServerCommand,
        runtime: StdioContext,
        progress: &super::progress::Progress,
    ) -> Result<Self> {
        progress.set("starting");
        let transport::StdioProcess {
            mut child,
            process_group,
            incoming,
            outgoing,
            stderr,
            debug_log,
        } = transport::spawn_stdio(&cwd, command, runtime.execution_path, runtime.debug_log)?;
        let stop = CancellationToken::new();
        let initialization_guard = stop.clone().drop_guard();
        let context = Arc::new(ClientContext {
            events: runtime.events,
            connection_id: debug_log.connection_id.clone(),
            bindings: Mutex::new(Default::default()),
            host: runtime.host,
            cwd: cwd.clone(),
            threads: runtime.threads,
            sessions: Mutex::new(HashMap::new()),
            pending_updates: Mutex::new(HashMap::new()),
            discarded: Mutex::new(Default::default()),
            replies: Mutex::new(HashMap::new()),
            stop: stop.clone(),
        });
        let (sender, received) = tokio::sync::oneshot::channel();
        let request_context = context.clone();
        let notification_context = context.clone();
        let lifetime = stop.clone();
        let exit_context = context.clone();
        let exit_log = debug_log.clone();
        let io = Client
            .builder()
            .name("aow")
            .on_receive_request(
                async move |request: UntypedMessage,
                            responder,
                            _connection: ConnectionTo<Agent>| {
                    request_context.handle_request(request, responder);
                    Ok(())
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_notification(
                async move |notification: UntypedMessage, _connection: ConnectionTo<Agent>| {
                    notification_context
                        .handle_notification(notification)
                        .map_err(|error| {
                            agent_client_protocol::Error::internal_error().data(error.to_string())
                        })
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .connect_with(
                Lines::new(outgoing, incoming),
                move |connection: ConnectionTo<Agent>| async move {
                    let _ = sender.send(connection);
                    futures::future::pending::<Result<(), agent_client_protocol::Error>>().await
                },
            );
        tokio::spawn(async move {
            let stderr_task = tokio::spawn(stderr);
            tokio::pin!(io);
            let reason = tokio::select! {
                result = &mut io => format!("ACP connection closed: {result:?}"),
                result = child.wait() => {
                    let _ = tokio::time::timeout(Duration::from_millis(250), &mut io).await;
                    format!("ACP adapter exited: {result:?}")
                },
                () = lifetime.cancelled() => "ACP connection closed".into(),
            };
            drop(process_group);
            let _ = child.kill().await;
            stderr_task.abort();
            lifetime.cancel();
            exit_context.disconnected(&reason);
            exit_log.archive();
        });
        let connection = tokio::time::timeout(Duration::from_secs(30), received)
            .await?
            .context("ACP adapter exited before connecting")?;
        progress.set("initializing");
        let response = tokio::time::timeout(
            Duration::from_secs(30),
            connection
                .send_request(
                    acp::InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(client_capabilities())
                        .client_info(acp::Implementation::new("aow", env!("CARGO_PKG_VERSION"))),
                )
                .block_task(),
        )
        .await;
        let response = match response {
            Ok(Ok(value)) => value,
            failure => {
                stop.cancel();
                anyhow::bail!("ACP initialization failed: {failure:?}");
            }
        };
        ensure!(
            response.protocol_version >= ProtocolVersion::V1,
            "Unsupported ACP protocol version"
        );
        let result = Self {
            info: ConnectionInfo {
                id: debug_log.connection_id.clone(),
                agent_id,
                cwd: cwd.to_string_lossy().into_owned(),
                agent_info: serde_json::to_value(response.agent_info)?,
                capabilities: serde_json::to_value(&response.agent_capabilities)?,
                auth_methods: serde_json::to_value(
                    crate::acp_thread::auth_methods::from_v1(response.auth_methods)?
                        .into_iter()
                        .filter(crate::acp_thread::auth_methods::is_supported)
                        .collect::<Vec<_>>(),
                )?,
                prompt_capabilities: serde_json::to_value(
                    crate::acp_thread::prompt_capabilities::from_v1(
                        response.agent_capabilities.prompt_capabilities.clone(),
                    ),
                )?,
            },
            connection,
            context,
            debug_log,
            loads: Default::default(),
            load_identity: Default::default(),
            stop,
        };
        initialization_guard.disarm();
        Ok(result)
    }
    pub(crate) fn connected(&self) -> bool {
        !self.stop.is_cancelled()
    }
    pub(crate) fn close(&self) {
        self.stop.cancel();
    }
    pub(crate) fn begin_prompt(
        &self,
        request: acp::PromptRequest,
    ) -> agent_client_protocol::SentRequest<acp::PromptResponse> {
        self.connection.send_request(request)
    }
    pub(crate) async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_inner(None, method, params).await
    }
    pub(crate) async fn thread_request(
        &self,
        thread_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        self.request_inner(Some(thread_id), method, params).await
    }
    async fn request_inner(
        &self,
        thread_id: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        ensure!(self.connected(), "ACP adapter is disconnected");
        let message = UntypedMessage::new(method, params)?;
        let pending = match thread_id {
            Some(thread_id) => self
                .debug_log
                .send_request(thread_id, || self.connection.send_request(message)),
            None => self.connection.send_request(message),
        };
        let started = std::time::Instant::now();
        let request_id = pending.id().clone();
        tracing::debug!(%request_id, method, thread_id, connection_id = %self.info.id, "ACP request started");
        let pending = pending.block_task();
        // Session opening is owned by the recovery task, not an RPC deadline.
        // Dropping its response future would cancel locally without rolling back the agent.
        let result = if matches!(
            method,
            "session/prompt" | "authenticate" | "session/load" | "session/resume"
        ) {
            pending.await.map_err(anyhow::Error::from)
        } else {
            match tokio::time::timeout(Duration::from_secs(60), pending).await {
                Ok(response) => response.map_err(anyhow::Error::from),
                Err(error) => {
                    tracing::warn!(%request_id, method, "ACP request timed out; SDK cancellation is not remote rollback");
                    Err(error.into())
                }
            }
        };
        tracing::debug!(%request_id, method, elapsed_ms = started.elapsed().as_millis(), success = result.is_ok(), "ACP request completed");
        result
    }

    pub(crate) fn cancel(&self, session_id: &str) -> Result<()> {
        self.context.cancel_permissions(session_id);
        self.connection
            .send_notification(acp::CancelNotification::new(acp::SessionId::new(
                session_id.to_owned(),
            )))?;
        Ok(())
    }
    pub(crate) fn supports(&self, capability: &str) -> bool {
        if capability == "load" {
            self.info.capabilities["loadSession"] == true
        } else {
            self.info.capabilities["sessionCapabilities"]
                .get(capability)
                .is_some_and(|value| !value.is_null() && value != false)
        }
    }
}

fn client_capabilities() -> acp::ClientCapabilities {
    // The wire contract is validated by the pinned SDK. No PTY/terminal authentication is advertised.
    serde_json::from_value(
        json!({"fs":{"readTextFile":true,"writeTextFile":true},"terminal":false,
        "session":{"configOptions":{"boolean":{}},"compaction":{},"notices":{}},
        "elicitation":{"form":{},"url":{}},"_meta":{"terminal_output":true}}),
    )
    .expect("static ACP capabilities are valid")
}
