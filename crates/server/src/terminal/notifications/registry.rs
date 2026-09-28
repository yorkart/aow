use super::TaskStopNotification;
pub(super) use aow_agents::sessions::tracking::SessionTarget as Target;
use aow_agents::sessions::{
    AgentSessionLocator, AgentSessionProvider, SessionRoots, tail::SessionTail,
    tracking::AgentSessionTracker,
};
use aow_protocol::TerminalAgentProcess;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

const MAX_SESSIONS: usize = 50;
const MAX_CANDIDATES: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    pub(super) agent: String,
    pub(super) process: Option<TerminalAgentProcess>,
    pub(super) cwd: String,
    pub(super) source_root: PathBuf,
    pub(super) target: Target,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct SessionKey {
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

pub(super) struct Binding {
    pub(super) identity: Identity,
    pub(super) sessions: HashSet<SessionKey>,
}

#[derive(Default)]
pub(super) struct Registry {
    pub(super) bindings: HashMap<String, Binding>,
    pub(super) readers: HashMap<SessionKey, SessionTail>,
}

impl Registry {
    pub(super) fn unchanged(&self, instance: &str, identity: &Identity) -> bool {
        self.bindings
            .get(instance)
            .is_some_and(|binding| binding.identity == *identity)
    }

    pub(super) fn unregister(&mut self, instance: &str) {
        if self.bindings.remove(instance).is_some() {
            self.release_unused();
        }
    }

    pub(super) fn retain_instances(&mut self, instances: &HashSet<String>) {
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
    pub(super) fn register(
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

    pub(super) fn poll(&mut self) -> Vec<TaskStopNotification> {
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

pub(super) fn candidates(identity: &Identity, roots: SessionRoots) -> Vec<AgentSessionLocator> {
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
