use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use aow_agents::sessions::{
    AgentSessionProvider, SessionEnvironment, SessionRoots,
    tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution},
};

use super::super::*;
use super::{POLL_INTERVAL, registry, sources::add_sources};

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

    pub(super) fn start_agent_notifications_with_environment(
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
            let mut registry = registry::Registry::default();
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
                for event in &events {
                    let _ = manager.inner.task_completions.send(event.clone());
                }
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
                        let identity = registry::Identity {
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
                            registry::candidates(&candidate_identity, roots)
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
