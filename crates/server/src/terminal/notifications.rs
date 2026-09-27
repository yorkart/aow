//! One binding per terminal instance, fixed candidates per registration, and
//! a shared budget of forward-only transcript readers.

use super::*;
use aow_agents::sessions::{
    AgentSessionLocator, AgentSessionProvider, SessionEnvironment, SessionRoots,
    tail::SessionTail,
    tracking::{
        AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget as Target,
    },
};
use aow_protocol::TerminalAgentProcess;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::{convert::Infallible, sync::atomic::Ordering};

const MAX_SESSIONS: usize = 50;
const MAX_CANDIDATES: usize = 5;
const POLL_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TaskStopNotification {
    pub(crate) agent: String,
    pub(crate) session_id: String,
    pub(crate) title: String,
    pub(crate) cwd: String,
    pub(crate) turn_id: Option<String>,
    pub(crate) conclusion: Option<String>,
    pub(crate) instance_ids: Vec<String>,
    pub(crate) sources: Vec<TaskStopSource>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TaskStopSource {
    pub(crate) project_name: String,
    pub(crate) workspace_root: String,
    pub(crate) tab_id: String,
    pub(crate) tab_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tab_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    agent: String,
    process: Option<TerminalAgentProcess>,
    cwd: String,
    source_root: PathBuf,
    target: Target,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct SessionKey {
    agent: &'static str,
    id: String,
    path: PathBuf,
}

impl From<&AgentSessionLocator> for SessionKey {
    fn from(locator: &AgentSessionLocator) -> Self {
        Self {
            agent: locator.agent,
            id: locator.session_id.clone(),
            path: locator.transcript_path.clone(),
        }
    }
}

struct Binding {
    identity: Identity,
    sessions: HashSet<SessionKey>,
}

#[derive(Default)]
struct Registry {
    bindings: HashMap<String, Binding>,
    readers: HashMap<SessionKey, SessionTail>,
}

impl Registry {
    fn unchanged(&self, instance: &str, identity: &Identity) -> bool {
        self.bindings
            .get(instance)
            .is_some_and(|binding| binding.identity == *identity)
    }

    fn unregister(&mut self, instance: &str) {
        if self.bindings.remove(instance).is_some() {
            self.release_unused();
        }
    }

    fn retain_instances(&mut self, instances: &HashSet<String>) {
        self.bindings.retain(|id, _| instances.contains(id));
        self.release_unused();
    }

    fn release_unused(&mut self) {
        self.readers.retain(|key, _| {
            self.bindings
                .values()
                .any(|binding| binding.sessions.contains(key))
        });
    }

    // Calling register again is an explicit refresh, even for the same title.
    // Automatic discovery calls it only after an identity change.
    fn register(
        &mut self,
        instance: String,
        identity: Identity,
        candidates: Vec<AgentSessionLocator>,
    ) {
        let mut selected = HashSet::new();
        for locator in candidates.into_iter().take(MAX_CANDIDATES) {
            let key = SessionKey::from(&locator);
            if let Some(reader) = self.readers.get_mut(&key) {
                reader.locator.title = locator.title;
            } else {
                match SessionTail::from_eof(locator) {
                    Ok(reader) => {
                        self.readers.insert(key.clone(), reader);
                    }
                    Err(error) => {
                        tracing::debug!(%error, session_id = %key.id, "cannot attach task-stop reader");
                        continue;
                    }
                }
            }
            selected.insert(key);
        }
        if selected.is_empty() {
            self.unregister(&instance);
            return;
        }
        tracing::info!(
            instance_id = %instance,
            agent = %identity.agent,
            session_count = selected.len(),
            has_process = identity.process.is_some(),
            "registered agent task-stop listener"
        );
        // A fresh registration for the same subscription refreshes its fixed
        // candidates for every owner, keeping the per-title cap at five even
        // when several terminals subscribe to that title at different times.
        for binding in self.bindings.values_mut() {
            if binding.identity.agent == identity.agent
                && binding.identity.cwd == identity.cwd
                && binding.identity.source_root == identity.source_root
                && binding.identity.target == identity.target
            {
                binding.sessions.clone_from(&selected);
            }
        }
        self.bindings.insert(
            instance,
            Binding {
                identity,
                sessions: selected,
            },
        );
        self.release_unused();
        if self.readers.len() > MAX_SESSIONS {
            for reader in self.readers.values_mut() {
                reader.refresh_modified();
            }
        }
        while self.readers.len() > MAX_SESSIONS {
            let oldest = self
                .readers
                .iter()
                .min_by(|(ka, a), (kb, b)| a.modified.cmp(&b.modified).then_with(|| ka.cmp(kb)))
                .map(|(key, _)| key.clone())
                .expect("over budget");
            self.readers.remove(&oldest);
            // Keep the binding. Budget eviction must not cause automatic
            // re-registration on the next metadata poll.
        }
    }

    fn poll(&mut self) -> Vec<TaskStopNotification> {
        let mut notifications = Vec::new();
        for (key, reader) in &mut self.readers {
            match reader.poll() {
                Ok(events) => {
                    if events.is_empty() {
                        continue;
                    }
                    // Titles can change without another OSC update/registration.
                    // Refresh only display metadata, once per delivering session;
                    // keep candidates, cursor and parser state untouched.
                    if let Some(title) = aow_agents::Agent::from_id(key.agent)
                        .and_then(aow_agents::Agent::sessions)
                        .and_then(|provider| provider.current_title(&reader.locator))
                    {
                        reader.locator.title = title;
                    }
                    let mut instance_ids: Vec<_> = self
                        .bindings
                        .iter()
                        .filter(|(_, binding)| binding.sessions.contains(key))
                        .map(|(id, _)| id.clone())
                        .collect();
                    instance_ids.sort();
                    for event in events {
                        notifications.push(TaskStopNotification {
                            agent: key.agent.to_owned(),
                            session_id: key.id.clone(),
                            title: reader.locator.title.clone(),
                            cwd: reader.locator.cwd.to_string_lossy().into_owned(),
                            turn_id: event.turn_id,
                            conclusion: event.conclusion,
                            instance_ids: instance_ids.clone(),
                            sources: Vec::new(),
                        });
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, session_id = %key.id, "task-stop read failed")
                }
            }
        }
        notifications
    }
}

impl TerminalManager {
    pub(crate) fn start_agent_notifications(&self, aow: crate::aow::AowManager) {
        let environment = aow_agents::KNOWN_AGENTS
            .iter()
            .flat_map(|agent| agent.definition().configuration_env.iter().copied())
            .chain(["HOME", "PATH"])
            .filter_map(|key| {
                std::env::var_os(key)
                    .filter(|value| !value.is_empty())
                    .map(|value| (key.to_owned(), PathBuf::from(value)))
            })
            .collect();
        self.start_agent_notifications_with_environment(environment, POLL_INTERVAL, aow);
    }

    fn start_agent_notifications_with_environment(
        &self,
        fallback_environment: SessionEnvironment,
        poll_interval: Duration,
        aow: crate::aow::AowManager,
    ) {
        if self
            .inner
            .notifications_started
            .swap(true, Ordering::Relaxed)
        {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        aow.notifications().start();
        tokio::spawn(async move {
            let mut registry = Registry::default();
            let mut interval = tokio::time::interval(poll_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let manager = TerminalManager { inner };
                let Ok(tabs) = manager.list_snapshot(None) else {
                    continue;
                };
                let instances: HashSet<_> = tabs
                    .tabs
                    .iter()
                    .flat_map(|tab| &tab.panes)
                    .map(|pane| pane.id.clone())
                    .collect();
                // Read before removing exited agents so a short-lived CLI's
                // final record is consumed; deleted shell instances are removed first.
                let result = tokio::task::spawn_blocking(move || {
                    registry.retain_instances(&instances);
                    let events = registry.poll();
                    (registry, events)
                })
                .await;
                let Ok((next, events)) = result else {
                    break;
                };
                registry = next;
                let detected = manager.agents(None).await;
                let mut events = events;
                // Resolve names at delivery time so renaming a project/tab
                // does not require re-registering or resetting a rollout tail.
                add_sources(&mut events, &tabs.tabs, detected.as_ref().ok(), &aow).await;
                for event in events {
                    tracing::info!(
                        agent = %event.agent,
                        session_id = %event.session_id,
                        subscribers = manager.inner.task_stops.receiver_count(),
                        "agent task stopped"
                    );
                    aow.notifications()
                        .dispatch(event, &manager.inner.task_stops);
                }
                let Ok(detected) = detected else {
                    continue;
                };
                for tab in &tabs.tabs {
                    for pane in &tab.panes {
                        let Some(agent) = detected.agents.get(&pane.id).and_then(|a| a.as_deref())
                        else {
                            registry.unregister(&pane.id);
                            continue;
                        };
                        let Some((provider, tracker)) = aow_agents::Agent::from_id(agent)
                            .and_then(|agent| agent.sessions().zip(agent.session_tracking()))
                        else {
                            registry.unregister(&pane.id);
                            continue;
                        };
                        let process = detected.processes.get(&pane.id);
                        let cwd = process.map_or(pane.cwd.as_str(), |process| &process.cwd);
                        if registry.bindings.get(&pane.id).is_some_and(|binding| {
                            binding.identity.agent != agent
                                || binding.identity.process.as_ref() != process
                                || binding.identity.cwd != cwd
                        }) {
                            registry.unregister(&pane.id);
                        }
                        // Older daemons omit PID metadata. The adapter decides
                        // whether the remaining title is sufficient to bind.
                        let environment = match process {
                            Some(process) => {
                                let Some(environment) = sessions::process_environment(process)
                                else {
                                    continue;
                                };
                                environment
                            }
                            None => fallback_environment.clone(),
                        };
                        let home = environment
                            .get("HOME")
                            .map_or(crate::PROCESS_HOME.as_path(), PathBuf::as_path);
                        let source_root = provider.session_root(home, &environment);
                        let target = match tracker
                            .resolve_live_session(LiveSessionContext {
                                pid: process.map(|process| process.pid),
                                cwd,
                                title: detected.titles.get(&pane.id).map_or("", String::as_str),
                                environment: &environment,
                            })
                            .await
                        {
                            SessionResolution::Resolved(target) => target,
                            SessionResolution::NotFound => {
                                registry.unregister(&pane.id);
                                continue;
                            }
                            SessionResolution::Unavailable => continue,
                        };
                        let identity = Identity {
                            agent: agent.to_owned(),
                            process: process.cloned(),
                            cwd: cwd.to_owned(),
                            source_root,
                            target,
                        };
                        if registry.unchanged(&pane.id, &identity) {
                            continue;
                        }
                        let roots = SessionRoots::from_configuration(home, &environment);
                        let candidate_identity = identity.clone();
                        let candidates = tokio::task::spawn_blocking(move || {
                            candidates(&candidate_identity, roots)
                        })
                        .await;
                        let Ok(candidates) = candidates else {
                            registry.unregister(&pane.id);
                            continue;
                        };
                        // Title/PID can precede the first persisted transcript.
                        // No candidates have been locked yet; retry discovery.
                        if candidates.is_empty() {
                            registry.unregister(&pane.id);
                            continue;
                        }
                        if process.is_some_and(|process| !sessions::same_process(process)) {
                            continue;
                        }
                        let instance = pane.id.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            registry.register(instance, identity, candidates);
                            registry
                        })
                        .await;
                        let Ok(next) = result else {
                            return;
                        };
                        registry = next;
                    }
                }
            }
        });
    }
}

async fn add_sources(
    events: &mut [TaskStopNotification],
    tabs: &[TerminalTab],
    detected: Option<&TerminalAgentList>,
    aow: &crate::aow::AowManager,
) {
    let mut project_names = HashMap::new();
    for event in events {
        for tab in tabs {
            if !tab
                .panes
                .iter()
                .any(|pane| event.instance_ids.contains(&pane.id))
            {
                continue;
            }
            let project_name = match project_names.get(&tab.workspace_root) {
                Some(name) => name,
                None => {
                    let name = aow
                        .project_name_for_workspace(&tab.workspace_root)
                        .await
                        .unwrap_or_else(|| {
                            Path::new(&tab.workspace_root)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or(&tab.workspace_root)
                                .to_owned()
                        });
                    project_names
                        .entry(tab.workspace_root.clone())
                        .or_insert(name)
                }
            };
            event.sources.push(TaskStopSource {
                project_name: project_name.clone(),
                workspace_root: tab.workspace_root.clone(),
                tab_id: tab.id.clone(),
                tab_name: displayed_tab_name(tab, detected),
                tab_url: None,
            });
        }
    }
}

// Keep the label consistent with frontend terminalTabPresentation: explicit
// names win, otherwise one agent supplies the title and several show a count.
fn displayed_tab_name(tab: &TerminalTab, detected: Option<&TerminalAgentList>) -> String {
    if tab.name_is_custom != Some(false) {
        return tab.name.clone();
    }
    let agents: Vec<_> = tab
        .panes
        .iter()
        .filter(|pane| {
            pane.status == TerminalPaneStatus::Running
                && detected
                    .and_then(|list| list.agents.get(&pane.id))
                    .unwrap_or(&pane.agent_id)
                    .is_some()
        })
        .collect();
    match agents.as_slice() {
        [pane] => detected
            .and_then(|list| list.titles.get(&pane.id))
            .filter(|title| !title.is_empty())
            .unwrap_or(&tab.name)
            .clone(),
        [] => tab.name.clone(),
        _ => format!("{} agents", agents.len()),
    }
}

fn candidates(identity: &Identity, roots: SessionRoots) -> Vec<AgentSessionLocator> {
    aow_agents::Agent::from_id(&identity.agent)
        .and_then(aow_agents::Agent::session_tracking)
        .map(|tracker| {
            tracker
                .candidate_sessions(&identity.target, Path::new(&identity.cwd), roots)
                .into_iter()
                .take(MAX_CANDIDATES)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) async fn events(State(state): State<AppState>) -> impl IntoResponse {
    let receiver = state.terminals.inner.task_stops.subscribe();
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(notification) => {
                    let event = Event::default()
                        .event("task-stopped")
                        .json_data(notification)
                        .expect("serializable notification");
                    return Some((Ok::<_, Infallible>(event), receiver));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    (
        [("X-Accel-Buffering", "no")],
        Sse::new(stream).keep_alive(KeepAlive::default()),
    )
}

#[cfg(test)]
mod tests;
