pub(crate) mod acp;
mod imports;
mod installation;
pub(crate) mod progress;

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
    connect_lock: tokio::sync::Mutex<()>,
    progress: Mutex<HashMap<(String, PathBuf), progress::Progress>>,
    installations: Mutex<HashMap<String, progress::Progress>>,
}

impl AgentServerStore {
    pub fn new(host: Arc<dyn AcpHost>, state_directory: Option<PathBuf>) -> Result<Self> {
        let cache = state_directory.clone().unwrap_or_else(|| {
            std::env::temp_dir().join(format!("aow-acp-{}", std::process::id()))
        });
        Ok(Self {
            inner: Arc::new(ServiceInner {
                host,
                registry: AgentRegistryStore::new(
                    state_directory
                        .as_ref()
                        .map(|path| path.join("registry.json")),
                ),
                cache,
                threads: Arc::new(Mutex::new(ThreadStore::new(
                    state_directory.map(|path| path.join("history")),
                )?)),
                connections: Mutex::new(HashMap::new()),
                configurations: Mutex::new(HashMap::new()),
                sessions: Mutex::new(HashMap::new()),
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
        let cwd = tokio::fs::canonicalize(cwd).await?;
        ensure!(cwd.is_dir(), "ACP working directory must be a directory");
        let progress = progress::Progress::default();
        self.inner
            .progress
            .lock()
            .unwrap()
            .insert((agent_id.into(), cwd.clone()), progress.clone());
        let result = self.connect_inner(agent_id, cwd, &progress).await;
        progress.finish(result.is_ok());
        result
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
        let configuration = format!("{launch_settings}\n{execution_path:?}");
        if let Some(connection) =
            self.inner
                .connections
                .lock()
                .unwrap()
                .values()
                .find(|connection| {
                    connection.info.agent_id == agent_id
                        && connection.info.cwd == cwd.to_string_lossy()
                        && connection.connected()
                        && self
                            .inner
                            .configurations
                            .lock()
                            .unwrap()
                            .get(&connection.info.id)
                            == Some(&configuration)
                })
        {
            return Ok(connection.info.clone());
        }
        let agent = settings
            .agent_servers
            .get(agent_id)
            .context("ACP Agent is not configured; open Settings → ACP")?;
        let command = get_command(
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
        let connection = Arc::new(
            AcpConnection::stdio(
                agent_id.into(),
                cwd,
                command,
                self.inner.host.clone(),
                execution_path,
                self.inner.threads.clone(),
                progress,
            )
            .await?,
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
        let result = connection
            .request(
                "session/new",
                json!({"cwd":connection.info.cwd,"mcpServers":[]}),
            )
            .await?;
        self.register_session(connection, result, None).await
    }
    async fn register_session(
        &self,
        connection: Arc<AcpConnection>,
        result: Value,
        existing_id: Option<String>,
    ) -> Result<SessionSnapshot> {
        let remote_id = result["sessionId"]
            .as_str()
            .context("Agent returned no session ID")?
            .to_owned();
        let id = existing_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut thread = AcpThread::new_snapshot(
            id.clone(),
            remote_id.clone(),
            connection.info.agent_id.clone(),
            connection.info.cwd.clone(),
        );
        if let Ok(previous) = self.snapshot(&id) {
            thread.title = previous.title;
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
            previous.context.unregister_session(&remote_id);
        }
        self.inner.threads.lock().unwrap().insert(thread)?;
        connection.context.register_session(&remote_id, &id)?;
        self.inner
            .sessions
            .lock()
            .unwrap()
            .insert(id.clone(), connection.info.id.clone());
        self.apply_defaults(&id).await?;
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
            self.record_error(id, &error)?;
        }
        let mut attempted = Vec::new();
        while let Some((config_id, value)) = next_default(
            &self.snapshot(id)?.config_options,
            &defaults,
            &mut attempted,
        ) {
            if let Err(error) = self.set_config_option(id, &config_id, value).await {
                self.record_error(id, &error)?;
            }
        }
        Ok(())
    }
    pub async fn load_session(
        &self,
        connection_id: &str,
        remote_id: &str,
    ) -> Result<SessionSnapshot> {
        let connection = self.connection(connection_id)?;
        ensure!(
            connection.supports("load"),
            "Agent does not support session loading"
        );
        let existing = self
            .inner
            .threads
            .lock()
            .unwrap()
            .threads
            .values()
            .find(|thread| {
                thread.agent_id == connection.info.agent_id
                    && thread.cwd == connection.info.cwd
                    && thread.remote_id == remote_id
            })
            .map(|thread| thread.id.clone());
        if let Some(id) = &existing {
            ensure!(
                self.snapshot(id)?.status != "working",
                "Cancel the current prompt before loading history"
            );
        }
        // Buffer replay until the load response, then install the new model atomically.
        let previous_mapping = connection
            .context
            .sessions
            .lock()
            .unwrap()
            .remove(remote_id);
        connection
            .context
            .pending_updates
            .lock()
            .unwrap()
            .remove(remote_id);
        let response = connection
            .request(
                "session/load",
                json!({"sessionId":remote_id,"cwd":connection.info.cwd,"mcpServers":[]}),
            )
            .await;
        let mut result = match response {
            Ok(result) => result,
            Err(error) => {
                connection
                    .context
                    .pending_updates
                    .lock()
                    .unwrap()
                    .remove(remote_id);
                if let Some(id) = previous_mapping {
                    connection.context.register_session(remote_id, &id)?;
                    if !connection.connected() {
                        self.inner.threads.lock().unwrap().update(&id, |thread| {
                            thread.status = "disconnected".into();
                        })?;
                    }
                }
                return Err(error);
            }
        };
        result["sessionId"] = remote_id.into();
        self.register_session(connection, result, existing).await
    }
    pub async fn resume(&self, id: &str) -> Result<SessionSnapshot> {
        let thread = self.snapshot(id)?;
        if let Ok(connection) = self.session_connection(id)
            && connection.connected()
            && thread.status != "disconnected"
        {
            return Ok(thread);
        }
        let info = self.connect(&thread.agent_id, thread.cwd.into()).await?;
        // Prefer history replay. Resume-only agents continue from the saved local transcript.
        let connection = self.connection(&info.id)?;
        if connection.supports("load") {
            return self.load_session(&info.id, &thread.remote_id).await;
        }
        ensure!(
            connection.supports("resume"),
            "Agent does not support session loading or resuming"
        );
        let result = connection
            .request(
                "session/resume",
                json!({"sessionId":thread.remote_id,"cwd":connection.info.cwd,"mcpServers":[]}),
            )
            .await?;
        let options = normalize_config_options(
            result
                .get("configOptions")
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or(json!([])),
        )?;
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.status = "idle".into();
            thread.needs_load = false;
            thread.error = None;
            thread.active_prompt = None;
            thread.modes = result.get("modes").cloned().unwrap_or(json!({}));
            thread.config_options = options;
            thread.config_options_supported = result
                .get("configOptions")
                .is_some_and(|value| !value.is_null());
            thread.auth_required = false;
        })?;
        connection.context.register_session(&thread.remote_id, id)?;
        self.inner
            .sessions
            .lock()
            .unwrap()
            .insert(id.into(), info.id);
        self.apply_defaults(id).await?;
        self.snapshot(id)
    }
    pub fn sessions(&self, cwd: Option<&str>) -> Vec<SessionInfo> {
        let mut threads: Vec<_> = self
            .inner
            .threads
            .lock()
            .unwrap()
            .threads
            .values()
            .filter(|thread| cwd.is_none_or(|cwd| cwd == thread.cwd))
            .map(SessionInfo::from)
            .collect();
        threads.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        threads
    }
    pub fn snapshot(&self, id: &str) -> Result<SessionSnapshot> {
        self.inner
            .threads
            .lock()
            .unwrap()
            .threads
            .get(id)
            .cloned()
            .context("ACP session not found")
    }
    pub fn logs(&self, connection_id: &str) -> Result<Vec<Value>> {
        Ok(self.connection(connection_id)?.debug_log.messages())
    }
    pub fn dismiss_notice(&self, id: &str, notice_id: &str) -> Result<()> {
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.notices.retain(|notice| notice.id != notice_id);
        })
    }
    pub fn session_logs(&self, id: &str) -> Result<Vec<Value>> {
        Ok(self.session_connection(id)?.debug_log.messages())
    }
    pub fn start_prompt(&self, id: &str, content: Vec<Content>) -> Result<()> {
        ensure!(!content.is_empty(), "Prompt is empty");
        let connection = self.session_connection(id)?;
        ensure!(connection.connected(), "ACP adapter is disconnected");
        let content = serde_json::to_value(content)?;
        // SDK validation stays internal to the stable facade.
        let blocks: Vec<agent_client_protocol::schema::v2::ContentBlock> =
            serde_json::from_value(content.clone())?;
        crate::acp_thread::content::validate_prompt_content_for_v1(&blocks)?;
        let mut threads = self.inner.threads.lock().unwrap();
        let thread = threads.threads.get(id).context("ACP session not found")?;
        ensure!(
            thread.status == "idle",
            "ACP session is busy or disconnected"
        );
        let remote_id = thread.remote_id.clone();
        let prompt_request = crate::acp_thread::content::prompt_to_v1(
            agent_client_protocol::schema::v2::PromptRequest::new(
                agent_client_protocol::schema::v2::SessionId::new(remote_id.clone()),
                blocks,
            ),
        )?;
        let turn_id = uuid::Uuid::new_v4().to_string();
        threads.update(id, |thread| {
            thread.active_prompt = Some(turn_id.clone());
            thread.status = "working".into();
            thread.error = None;
            thread.stop_reason = None;
            thread.pending_user_echo = content
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|block| block["text"].as_str())
                .collect::<String>();
            for block in content.as_array().into_iter().flatten() {
                AcpThread::push(thread, "user", block.clone());
            }
            if thread.title == "New conversation" {
                thread.title = content[0]["text"]
                    .as_str()
                    .unwrap_or("Conversation")
                    .chars()
                    .take(80)
                    .collect();
            }
        })?;
        drop(threads);
        // Enqueue before returning so an immediate cancel cannot overtake this prompt.
        let pending = connection.begin_prompt(prompt_request);
        let service = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let result = pending.block_task().await.and_then(|result| {
                serde_json::to_value(result)
                    .map_err(agent_client_protocol::Error::into_internal_error)
            });
            let mut threads = service.inner.threads.lock().unwrap();
            if !threads
                .threads
                .get(&id)
                .is_some_and(|thread| thread.active_prompt.as_ref() == Some(&turn_id))
            {
                return;
            }
            let saved = threads.update(&id, |thread| {
                thread.active_prompt = None;
                thread.pending_user_echo.clear();
                thread.status = if connection.connected() && thread.status != "disconnected" {
                    "idle"
                } else {
                    "disconnected"
                }
                .into();
                match result {
                    Ok(result) => {
                        thread.stop_reason = result["stopReason"].as_str().map(str::to_owned);
                        if let Some(usage) = result.get("usage") {
                            thread.usage = usage.clone();
                        }
                    }
                    Err(error) => {
                        thread.auth_required =
                            error.code == agent_client_protocol::ErrorCode::AuthRequired;
                        thread.error = Some(format!("{error:#}"));
                    }
                }
            });
            if let Err(error) = saved {
                tracing::error!(%error, "Failed to save ACP prompt result");
            }
        });
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        self.session_connection(id)?
            .cancel(&self.snapshot(id)?.remote_id)
    }
    pub async fn set_mode(&self, id: &str, mode: &str) -> Result<()> {
        let thread = self.snapshot(id)?;
        self.session_connection(id)?
            .request(
                "session/set_mode",
                json!({"sessionId":thread.remote_id,"modeId":mode}),
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
            self.snapshot(id)?.remote_id,
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
        let thread = self.snapshot(id)?;
        if let Ok(connection) = self.session_connection(id)
            && connection.connected()
        {
            if thread.status == "working" {
                connection.cancel(&thread.remote_id)?;
            }
            if connection.supports("close") {
                connection
                    .request("session/close", json!({"sessionId":thread.remote_id}))
                    .await?;
            }
        }
        self.detach_session(id, &thread.remote_id)
    }
    fn detach_session(&self, id: &str, remote_id: &str) -> Result<()> {
        if let Ok(connection) = self.session_connection(id) {
            connection.context.unregister_session(remote_id);
        }
        self.inner.sessions.lock().unwrap().remove(id);
        self.inner.threads.lock().unwrap().update(id, |thread| {
            thread.status = "disconnected".into();
            thread.active_prompt = None;
            thread.permissions.clear();
        })
    }
    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let thread = self.snapshot(id)?;
        if let Ok(connection) = self.session_connection(id)
            && connection.connected()
            && connection.supports("delete")
        {
            if thread.status == "working" {
                connection.cancel(&thread.remote_id)?;
            }
            connection
                .request("session/delete", json!({"sessionId":thread.remote_id}))
                .await?;
            self.detach_session(id, &thread.remote_id)?;
        } else {
            self.close_session(id).await?;
        }
        self.inner.threads.lock().unwrap().remove(id)
    }
    fn record_error(&self, id: &str, error: &anyhow::Error) -> Result<()> {
        self.inner
            .threads
            .lock()
            .unwrap()
            .update(id, |thread| thread.error = Some(format!("{error:#}")))
    }
}
