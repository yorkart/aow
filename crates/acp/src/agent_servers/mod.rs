pub(crate) mod acp;
mod completion;
mod identity;
mod imports;
mod installation;
mod lifecycle;
pub(crate) mod progress;
mod prompt;
mod recovery;
mod steering;
mod sync;

use crate::acp_thread::normalize_config_options;
use crate::facade::{AcpHost, AgentInfo, ConnectionInfo, Content, SessionInfo, SessionSnapshot};
use crate::{
    acp_thread::{AcpThread, defaults::next_default, history::ThreadStore},
    agent_servers::acp::AcpConnection,
    project::{
        agent_registry_store::{AgentRegistryStore, current_platform_key},
        agent_server_store::get_command,
    },
    settings::validate_settings,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct AgentServerStore {
    inner: Arc<ServiceInner>,
}
struct ServiceInner {
    host: Arc<dyn AcpHost>,
    cache: PathBuf,
    registry: AgentRegistryStore,
    threads: Arc<Mutex<ThreadStore>>,
    connections: Mutex<HashMap<String, Arc<AcpConnection>>>,
    configurations: Mutex<HashMap<String, String>>,
    sessions: Mutex<HashMap<String, String>>,
    logs: acp::AcpDebugLogStore,
    connect_lock: tokio::sync::Mutex<()>,
    progress: Mutex<HashMap<(String, PathBuf), progress::Progress>>,
    installations: Mutex<HashMap<String, progress::Progress>>,
    events: crate::facade::events::SyncEvents,
}

impl AgentServerStore {
    pub fn new(host: Arc<dyn AcpHost>, state_directory: Option<PathBuf>) -> Result<Self> {
        let cache = state_directory.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("aow-acp-{}", std::process::id()))
        });
        let metadata_host = host.clone();
        let threads = ThreadStore::new(state_directory.clone(), move || {
            metadata_host.session_metadata_changed();
        })?;
        let events = threads.events.clone();
        Ok(Self {
            inner: Arc::new(ServiceInner {
                host,
                registry: AgentRegistryStore::new(
                    state_directory
                        .as_ref()
                        .map(|path| path.join("registry.json")),
                ),
                cache,
                threads: Arc::new(Mutex::new(threads)),
                events,
                connections: Mutex::new(HashMap::new()),
                configurations: Mutex::new(HashMap::new()),
                sessions: Mutex::new(HashMap::new()),
                logs: acp::AcpDebugLogStore::default(),
                connect_lock: tokio::sync::Mutex::new(()),
                progress: Mutex::new(HashMap::new()),
                installations: Mutex::new(HashMap::new()),
            }),
        })
    }
    pub async fn agents(&self) -> Result<Vec<AgentInfo>> {
        let raw = self.inner.host.settings().await?;
        let settings = validate_settings(&raw)?;
        let values: Value = serde_json_lenient::from_str(&raw)?;
        let execution_path = self.inner.host.execution_path().await?;
        Ok(settings
            .agent_servers
            .keys()
            .map(|id| AgentInfo {
                id: id.clone(),
                name: id.clone(),
                description: String::new(),
                version: None,
                configured: true,
                supported: true,
                installed: self.installed(id, &values, execution_path.as_deref()),
            })
            .collect())
    }
    pub async fn registry(&self, refresh: bool) -> Result<Vec<AgentInfo>> {
        let configured = self.agents().await?;
        Ok(self
            .inner
            .registry
            .agents(refresh)
            .await?
            .into_iter()
            .map(|entry| {
                let supported = entry.distribution.npx.is_some()
                    || current_platform_key().is_some_and(|key| {
                        entry
                            .distribution
                            .binary
                            .as_ref()
                            .is_some_and(|targets| targets.contains_key(key))
                    });
                AgentInfo {
                    configured: configured.iter().any(|agent| agent.id == entry.id),
                    installed: configured
                        .iter()
                        .any(|agent| agent.id == entry.id && agent.installed),
                    id: entry.id,
                    name: entry.name,
                    description: entry.description,
                    version: Some(entry.version),
                    supported,
                }
            })
            .collect())
    }
    pub async fn connect(&self, agent_id: &str, cwd: PathBuf) -> Result<ConnectionInfo> {
        self.connect_for_thread(agent_id, cwd, None)
            .await
            .map(|(info, _)| info)
    }
    async fn connect_for_thread(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        thread_id: Option<&str>,
    ) -> Result<(ConnectionInfo, progress::Progress)> {
        let cwd = tokio::fs::canonicalize(cwd).await?;
        ensure!(cwd.is_dir(), "ACP working directory must be a directory");
        let progress = progress::Progress::observed(
            self.inner.events.clone(),
            crate::facade::SyncResource::Operation {
                agent_id: agent_id.into(),
                cwd: Some(cwd.to_string_lossy().into_owned()),
            },
        );
        self.inner
            .progress
            .lock()
            .unwrap()
            .insert((agent_id.into(), cwd.clone()), progress.clone());
        progress.notify();
        let result = self
            .connect_inner(agent_id, cwd, &progress, thread_id)
            .await;
        if thread_id.is_none() || result.is_err() {
            progress.finish(result.is_ok());
        } else {
            progress.set("restoring");
        }
        result.map(|info| (info, progress))
    }
    pub async fn connection_status(
        &self,
        agent_id: &str,
        cwd: PathBuf,
    ) -> Result<Option<crate::facade::ConnectionStatus>> {
        let cwd = tokio::fs::canonicalize(cwd).await?;
        Ok(self
            .inner
            .progress
            .lock()
            .unwrap()
            .get(&(agent_id.into(), cwd))
            .map(progress::Progress::snapshot))
    }
    async fn connect_inner(
        &self,
        agent_id: &str,
        cwd: PathBuf,
        progress: &progress::Progress,
        thread_id: Option<&str>,
    ) -> Result<ConnectionInfo> {
        let _guard = self.inner.connect_lock.lock().await;
        progress.set("configuration");
        let raw_settings = self.inner.host.settings().await?;
        let settings = validate_settings(&raw_settings)?;
        let settings_json: Value = serde_json_lenient::from_str(&raw_settings)?;
        let execution_path = self.inner.host.execution_path().await?;
        let mut launch_settings = settings_json["agent_servers"][agent_id].clone();
        if let Some(fields) = launch_settings.as_object_mut() {
            // Composer preferences apply to sessions, not the adapter process.
            for key in [
                "default_mode",
                "default_config_options",
                "favorite_config_option_values",
            ] {
                fields.remove(key);
            }
        }
        let environment = self.inner.host.execution_environment();
        let configuration = format!("{launch_settings}\n{execution_path:?}\n{environment:?}");
        // Do not hold the connection registry while consulting a session operation:
        // committing a load also updates the thread-to-connection association.
        let preferred = thread_id.and_then(|id| self.session_connection(id).ok());
        let mut candidates: Vec<_> = self
            .inner
            .connections
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        candidates.sort_by_key(|candidate| {
            preferred
                .as_ref()
                .is_none_or(|connection| connection.info.id != candidate.info.id)
        });
        if let Some(connection) = candidates.iter().find(|connection| {
            connection.info.agent_id == agent_id
                && connection.info.cwd == cwd.to_string_lossy()
                && connection.connected()
                && thread_id.is_none_or(|id| !connection.loads.blocks_thread(id))
                && self
                    .inner
                    .configurations
                    .lock()
                    .unwrap()
                    .get(&connection.info.id)
                    == Some(&configuration)
        }) {
            if let Some(thread_id) = thread_id {
                connection.debug_log.attach(thread_id);
            }
            return Ok(connection.info.clone());
        }
        let agent = settings
            .agent_servers
            .get(agent_id)
            .context("ACP Agent is not configured; open Settings → ACP")?;
        let mut command = get_command(
            agent_id,
            agent,
            &self.inner.registry,
            &self.inner.cache.join("agents"),
            execution_path.as_deref(),
            progress,
        )
        .await?;
        self.record_installation(
            agent_id,
            &settings_json,
            execution_path.as_deref(),
            &command,
        )?;
        command.env.extend(environment);
        let debug_log = self.inner.logs.create_log();
        if let Some(thread_id) = thread_id {
            debug_log.attach(thread_id);
        }
        let connection = Arc::new(
            AcpConnection::stdio(
                agent_id.into(),
                cwd,
                command,
                acp::StdioContext {
                    events: self.inner.events.clone(),
                    host: self.inner.host.clone(),
                    execution_path,
                    threads: self.inner.threads.clone(),
                    debug_log: debug_log.clone(),
                },
                progress,
            )
            .await
            .inspect_err(|_| debug_log.archive())?,
        );
        let info = connection.info.clone();
        self.inner
            .configurations
            .lock()
            .unwrap()
            .insert(info.id.clone(), configuration);
        self.inner
            .connections
            .lock()
            .unwrap()
            .insert(info.id.clone(), connection);
        Ok(info)
    }
    fn connection(&self, id: &str) -> Result<Arc<AcpConnection>> {
        self.inner
            .connections
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .context("ACP connection not found")
    }
    fn session_connection(&self, id: &str) -> Result<Arc<AcpConnection>> {
        let connection_id = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .context("ACP session is disconnected; resume it explicitly")?;
        self.connection(&connection_id)
    }
    pub fn connection_requests(&self, id: &str) -> Result<Vec<crate::facade::Permission>> {
        Ok(self.connection(id)?.context.requests())
    }
    pub fn answer_connection(&self, id: &str, request_id: &str, response: Value) -> Result<()> {
        self.connection(id)?.context.answer(request_id, response)
    }
    pub fn disconnect(&self, id: &str) -> Result<()> {
        self.connection(id)?.close();
        Ok(())
    }
    pub async fn authenticate(&self, connection_id: &str, method_id: &str) -> Result<()> {
        self.connection(connection_id)?
            .request("authenticate", json!({"methodId":method_id}))
            .await?;
        let ids: Vec<_> = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, connection)| connection.as_str() == connection_id)
            .map(|(id, _)| id.clone())
            .collect();
        let mut threads = self.inner.threads.lock().unwrap();
        for id in ids {
            threads.update(&id, |thread| {
                if thread.auth_required {
                    thread.error = None;
                    thread.auth_required = false;
                }
            })?;
        }
        Ok(())
    }
    pub async fn remote_sessions(
        &self,
        connection_id: &str,
        cursor: Option<String>,
    ) -> Result<Value> {
        let connection = self.connection(connection_id)?;
        ensure!(
            connection.supports("list"),
            "Agent does not support session listing"
        );
        connection
            .request(
                "session/list",
                json!({"cwd":connection.info.cwd,"cursor":cursor}),
            )
            .await
    }
    pub async fn new_session(&self, connection_id: &str) -> Result<SessionSnapshot> {
        let connection = self.connection(connection_id)?;
        let thread = self
            .prepare_session(
                &connection.info.agent_id,
                connection.info.cwd.clone().into(),
            )
            .await?;
        self.create_on_connection(connection, &thread.thread_id)
            .await
    }
    async fn register_session(
        &self,
        connection: Arc<AcpConnection>,
        result: Value,
        id: String,
    ) -> Result<SessionSnapshot> {
        self.install_session(connection, result, id.clone())?;
        self.apply_defaults(&id).await?;
        self.snapshot(&id)
    }
    fn install_session(
        &self,
        connection: Arc<AcpConnection>,
        result: Value,
        id: String,
    ) -> Result<SessionSnapshot> {
        let session_id = result["sessionId"]
            .as_str()
            .context("Agent returned no session ID")?
            .to_owned();
        let mut thread = AcpThread::new_snapshot(
            id.clone(),
            session_id.clone(),
            connection.info.agent_id.clone(),
            connection.info.cwd.clone(),
        );
        let previous = self.snapshot(&id).ok();
        if let Some(previous) = &previous {
            thread.title = previous.title.clone();
            thread.revision = previous.revision;
        }
        thread.modes = result.get("modes").cloned().unwrap_or(json!({}));
        thread.config_options_supported = result
            .get("configOptions")
            .is_some_and(|value| !value.is_null());
        thread.config_options = normalize_config_options(
            result
                .get("configOptions")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or(json!([])),
        )?;
        if let Ok(previous) = self.session_connection(&id)
            && previous.info.id != connection.info.id
        {
            previous.context.bindings.lock().unwrap().remove(&id);
            previous.context.unregister_session(&session_id);
        }
        connection
            .context
            .install_session(thread, previous.as_ref())?;
        connection.debug_log.bind_session(&session_id, &id);
        self.attach_connection(&connection, &id)?;
        self.snapshot(&id)
    }
    async fn apply_defaults(&self, id: &str) -> Result<()> {
        let thread = self.snapshot(id)?;
        let settings = validate_settings(&self.inner.host.settings().await?)?;
        let Some(agent) = settings.agent_servers.get(&thread.agent_id) else {
            return Ok(());
        };
        let (mode, defaults) = agent.defaults();
        let _favorites = agent.favorites();
        if let Some(mode) = mode
            && thread.modes["availableModes"]
                .as_array()
                .is_some_and(|modes| modes.iter().any(|entry| entry["id"] == mode))
            && let Err(error) = self.set_mode(id, &mode).await
        {
            self.record_recovery_error(id, &error)?;
        }
        let mut attempted = Vec::new();
        while let Some((config_id, value)) = next_default(
            &self.snapshot(id)?.config_options,
            &defaults,
            &mut attempted,
        ) {
            if let Err(error) = self.set_config_option(id, &config_id, value).await {
                self.record_recovery_error(id, &error)?;
            }
        }
        Ok(())
    }
    pub async fn load_session(
        &self,
        connection_id: &str,
        session_id: &str,
    ) -> Result<SessionSnapshot> {
        let connection = self.connection(connection_id)?;
        ensure!(
            connection.supports("load"),
            "Agent does not support session loading"
        );
        let identity = connection.load_identity.lock().await;
        let existing = self
            .inner
            .threads
            .lock()
            .unwrap()
            .metadata
            .entry_by_session(&connection.info.agent_id, &connection.info.cwd, session_id)
            .map(|thread| thread.thread_id.clone());
        if let Some(id) = &existing {
            ensure!(
                self.snapshot(id)?.status != "working",
                "Cancel the current prompt before loading history"
            );
        }
        let id = match existing {
            Some(id) => id,
            None => {
                let thread = self
                    .prepare_session(
                        &connection.info.agent_id,
                        connection.info.cwd.clone().into(),
                    )
                    .await?;
                self.inner
                    .threads
                    .lock()
                    .unwrap()
                    .update(&thread.thread_id, |thread| {
                        thread.session_id = Some(session_id.into());
                        thread.needs_load = true;
                    })?;
                thread.thread_id
            }
        };
        drop(identity);
        self.open_session(connection, session_id.to_owned(), id, true)
            .await
    }
    pub async fn resume(&self, id: &str) -> Result<SessionSnapshot> {
        let result = self.resume_inner(id).await;
        if let Err(error) = &result {
            let thread = self.snapshot(id)?;
            // A started operation publishes its own error, scoped to its attempt.
            // Connection/setup failures have no operation yet.
            if thread.recovery.is_none() && (thread.status == "disconnected" || thread.needs_load) {
                self.record_recovery_error(id, error)?;
            }
        }
        result
    }
    async fn resume_inner(&self, id: &str) -> Result<SessionSnapshot> {
        let thread = self.snapshot(id)?;
        if let Ok(connection) = self.session_connection(id)
            && connection.connected()
            && thread.status != "disconnected"
            && !thread.needs_load
            && thread.session_id.is_some()
            && !connection.loads.pending_thread(id)
            && !connection.loads.blocks_thread(id)
        {
            return Ok(thread);
        }
        let (info, progress) = self
            .connect_for_thread(&thread.agent_id, thread.cwd.into(), Some(id))
            .await?;
        let result = async {
            // Prefer history replay. Resume-only agents continue from the saved local transcript.
            let connection = self.connection(&info.id)?;
            let Some(session_id) = thread.session_id else {
                return self.create_on_connection(connection, id).await;
            };
            connection.debug_log.bind_session(&session_id, id);
            if connection.supports("load") {
                return self.load_session(&info.id, &session_id).await;
            }
            ensure!(
                connection.supports("resume"),
                "Agent does not support session loading or resuming"
            );
            self.open_session(connection, session_id, id.into(), false)
                .await
        }
        .await;
        progress.finish(result.is_ok());
        result
    }
    // agent::NativeAgentServer::flush_threads_on_quit, adapted to the host snapshot cache.
    pub fn flush_threads_on_quit(&self) -> Result<()> {
        self.inner.threads.lock().unwrap().metadata.flush()
    }
    pub fn sessions(&self, cwd: Option<&str>) -> Vec<SessionInfo> {
        self.inner.threads.lock().unwrap().sessions(cwd)
    }
    pub fn snapshot(&self, id: &str) -> Result<SessionSnapshot> {
        let thread = self.inner.threads.lock().unwrap().thread(id)?;
        Ok(thread.lock().unwrap().clone())
    }
    pub fn updates(
        &self,
        id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<crate::facade::SessionUpdates> {
        let thread = self.inner.threads.lock().unwrap().thread(id)?;
        Ok(crate::acp_thread::updates::since(
            &thread.lock().unwrap(),
            generation,
            revision,
        ))
    }
    pub fn overview(&self, id: &str) -> Result<SessionSnapshot> {
        let thread = self.inner.threads.lock().unwrap().thread(id)?;
        Ok(crate::acp_thread::overview::snapshot(
            &mut thread.lock().unwrap(),
        ))
    }
    pub fn overview_updates(
        &self,
        id: &str,
        generation: &str,
        revision: u64,
    ) -> Result<crate::facade::SessionUpdates> {
        let thread = self.inner.threads.lock().unwrap().thread(id)?;
        Ok(crate::acp_thread::overview::since(
            &mut thread.lock().unwrap(),
            generation,
            revision,
        ))
    }
    pub fn process(
        &self,
        id: &str,
        turn_id: &str,
        generation: Option<&str>,
        revision: Option<u64>,
    ) -> Result<crate::facade::SessionUpdates> {
        let thread = self.inner.threads.lock().unwrap().thread(id)?;
        crate::acp_thread::overview::process(
            &mut thread.lock().unwrap(),
            turn_id,
            generation,
            revision,
        )
    }
    pub fn logs(&self, connection_id: &str) -> Result<Vec<Value>> {
        // Initialization can fail before a live AcpConnection is registered.
        self.inner
            .logs
            .messages(connection_id)
            .context("ACP connection log expired or not found")
    }
    pub fn dismiss_notice(&self, id: &str, notice_id: &str) -> Result<()> {
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.notices.retain(|notice| notice.id != notice_id);
        })
    }
    pub fn clear_plan(&self, id: &str) -> Result<()> {
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.plan = Value::Null;
        })
    }
    pub fn session_logs(&self, id: &str) -> Result<Vec<Value>> {
        self.session_info(id)?;
        let mut messages = self.inner.logs.thread_messages(id);
        messages.sort_by(|a, b| a["timestamp"].as_str().cmp(&b["timestamp"].as_str()));
        Ok(messages)
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        if self.cancel_recovery(id)? {
            return Ok(());
        }
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.steering.cancelled = true;
            if let Some(pending) = &mut thread.steering.pending {
                pending.cancelled = true;
            }
            if thread
                .steering
                .pending
                .as_ref()
                .is_some_and(|pending| pending.prompt_required)
            {
                let pending = thread.steering.pending.take().unwrap();
                crate::acp_thread::steering::delivery(
                    thread,
                    pending.entry_start,
                    "cancelled",
                    None,
                );
                thread.steering_pending = false;
                crate::acp_thread::steering::settle_status(thread);
            }
        })?;
        self.session_connection(id)?.cancel(
            self.snapshot(id)?
                .session_id
                .as_deref()
                .context("ACP session has not been created")?,
        )
    }
    pub async fn set_mode(&self, id: &str, mode: &str) -> Result<()> {
        let thread = self.snapshot(id)?;
        let session_id = thread
            .session_id
            .context("ACP session has not been created")?;
        self.session_connection(id)?
            .request(
                "session/set_mode",
                json!({"sessionId":session_id,"modeId":mode}),
            )
            .await?;
        self.inner
            .threads
            .lock()
            .unwrap()
            .update(id, |thread| thread.modes["currentModeId"] = mode.into())
    }
    pub async fn set_config_option(&self, id: &str, config_id: &str, value: Value) -> Result<()> {
        ensure!(
            value.is_string() || value.is_boolean(),
            "ACP config values must be strings or booleans"
        );
        let option_value = if let Some(value) = value.as_bool() {
            agent_client_protocol::schema::v2::SessionConfigOptionValue::boolean(value)
        } else {
            agent_client_protocol::schema::v2::SessionConfigOptionValue::id(
                value.as_str().context("Invalid option")?,
            )
        };
        let value = crate::acp_thread::config_options::value_to_v1(option_value)?;
        let request = agent_client_protocol::schema::v1::SetSessionConfigOptionRequest::new(
            self.snapshot(id)?
                .session_id
                .context("ACP session has not been created")?,
            config_id.to_owned(),
            value,
        );
        let result = self
            .session_connection(id)?
            .request("session/set_config_option", serde_json::to_value(request)?)
            .await?;
        let options = normalize_config_options(result["configOptions"].clone())?;
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.config_options = options;
        })
    }
    pub fn answer(&self, id: &str, request_id: &str, response: Value) -> Result<()> {
        let thread = self.snapshot(id)?;
        let permission = thread
            .permissions
            .iter()
            .find(|request| request.id == request_id)
            .context("Request is no longer pending")?;
        if permission.kind == "permission" {
            let _: agent_client_protocol::schema::v1::RequestPermissionResponse =
                serde_json::from_value(response.clone())?;
            if response["outcome"]["outcome"] == "selected" {
                ensure!(
                    permission.request["options"]
                        .as_array()
                        .is_some_and(|options| options
                            .iter()
                            .any(|option| option["optionId"] == response["outcome"]["optionId"])),
                    "Unknown permission option"
                );
            }
        }
        if permission.kind == "elicitation" {
            let _: agent_client_protocol::schema::v1::CreateElicitationResponse =
                serde_json::from_value(response.clone())?;
        }
        self.session_connection(id)?
            .context
            .answer(request_id, response)
    }
    pub async fn close_session(&self, id: &str) -> Result<()> {
        if self.close_pending_load(id).await? {
            return Ok(());
        }
        let thread = self.snapshot(id)?;
        if let Ok(connection) = self.session_connection(id)
            && connection.connected()
            && let Some(session_id) = &thread.session_id
        {
            if thread.status == "working" {
                connection.cancel(session_id)?;
            }
            if connection.supports("close") {
                connection
                    .request("session/close", json!({"sessionId":thread.session_id}))
                    .await?;
            }
        }
        self.detach_session(id, thread.session_id.as_deref())
    }
    fn detach_session(&self, id: &str, session_id: Option<&str>) -> Result<()> {
        if let Ok(connection) = self.session_connection(id) {
            connection.context.bindings.lock().unwrap().remove(id);
            if let Some(session_id) = session_id {
                connection.context.unregister_session(session_id);
                connection.loads.forget(session_id);
            }
        }
        self.inner.sessions.lock().unwrap().remove(id);
        self.inner
            .events
            .changed(crate::facade::SyncResource::Connection {
                thread_id: id.into(),
            });
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.status = "disconnected".into();
            crate::acp_thread::steering::disconnected(thread);
            thread.active_prompt = None;
            thread.permissions.clear();
        })
    }
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        self.close_pending_load(id).await?;
        let thread = self.snapshot(id)?;
        if let Ok(connection) = self.session_connection(id)
            && connection.connected()
            && connection.supports("delete")
            && let Some(session_id) = &thread.session_id
        {
            if thread.status == "working" {
                connection.cancel(session_id)?;
            }
            connection
                .request("session/delete", json!({"sessionId":thread.session_id}))
                .await?;
            self.detach_session(id, Some(session_id))?;
        } else {
            self.close_session(id).await?;
        }
        self.inner.threads.lock().unwrap().remove(id)?;
        self.inner.logs.remove_thread(id);
        Ok(())
    }
}
