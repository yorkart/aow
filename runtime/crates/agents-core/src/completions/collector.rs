use super::*;
use crate::catalog::SessionCatalog;
use crate::sessions::{AgentSessionProvider, process::same_process};
use std::{collections::HashSet, time::Duration};

impl Completions {
    pub fn start(
        &self,
        sessions: SessionCatalog,
        mut stopping: watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let service = self.clone();
        aow_i18n::spawn(async move {
            let mut registry = registry::Registry::default();
            let mut pending = Vec::new();
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            'collection: loop {
                tokio::select! { _=interval.tick()=>{}, _=stopping.changed()=>break }
                if !pending.is_empty() {
                    let log = service.clone();
                    let batch = pending.clone();
                    match aow_i18n::spawn_blocking(move || log.append(&batch)).await {
                        Ok(Ok(())) => pending.clear(),
                        error => {
                            tracing::error!(?error, "cannot persist native completions; retrying");
                            continue;
                        }
                    }
                }
                // Read first, even when a CLI has just exited.
                let output = aow_i18n::spawn_blocking(move || {
                    let events = registry.poll();
                    (registry, events)
                })
                .await;
                let Ok((next, events)) = output else { return };
                registry = next;
                pending = events;
                let service_for_log = service.clone();
                let batch = pending.clone();
                if let Err(error) = aow_i18n::spawn_blocking(move || service_for_log.append(&batch))
                    .await
                    .unwrap_or_else(|error| Err(error.into()))
                {
                    // A durable log failure must be visible; never acknowledge an unlogged event.
                    tracing::error!(%error,"cannot persist native completions");
                } else {
                    pending.clear();
                }
                let Ok(runtimes) = sessions.observer().runtimes() else {
                    continue;
                };
                let instances: HashSet<_> = runtimes
                    .runtimes
                    .into_iter()
                    .map(|runtime| runtime.id)
                    .collect();
                registry.retain_instances(&instances);
                let detected = tokio::select! { detected=sessions.observer().agents()=>detected, _=stopping.changed()=>break };
                let Ok(detected) = detected else {
                    continue;
                };
                for instance in instances {
                    if registry.bindings.get(&instance).is_some_and(|binding| {
                        detected
                            .agents
                            .get(&instance)
                            .and_then(|agent| agent.as_deref())
                            != Some(binding.identity.agent.as_str())
                            || detected.processes.get(&instance)
                                != binding.identity.process.as_ref()
                    }) {
                        registry.unregister(&instance);
                    }
                    let binding = match tokio::select! {binding=sessions.binding(&instance)=>binding,_=stopping.changed()=>break 'collection}
                    {
                        Ok(Some(binding)) => binding,
                        Ok(None) => {
                            registry.unregister(&instance);
                            continue;
                        }
                        Err(_) => continue,
                    };
                    let Some(target) = binding.target else {
                        registry.unregister(&instance);
                        continue;
                    };
                    let provider = crate::Agent::from_id(binding.agent)
                        .and_then(crate::Agent::sessions)
                        .unwrap();
                    let home = binding
                        .environment
                        .get("HOME")
                        .cloned()
                        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
                        .unwrap_or_default();
                    let identity = registry::Identity {
                        agent: binding.agent.into(),
                        process: Some(binding.process.clone()),
                        cwd: binding.cwd,
                        source_root: provider.session_root(&home, &binding.environment),
                        target,
                    };
                    if registry.unchanged(&instance, &identity)
                        && !(identity.agent == "codex"
                            && matches!(identity.target, registry::Target::Id(_)))
                    {
                        continue;
                    }
                    let identity_for_candidates = identity.clone();
                    let Ok(candidates) = aow_i18n::spawn_blocking(move || {
                        registry::candidates(&identity_for_candidates, binding.roots)
                    })
                    .await
                    else {
                        continue;
                    };
                    if !same_process(&binding.process) {
                        continue;
                    }
                    let Ok(next) = aow_i18n::spawn_blocking(move || {
                        registry.refresh(instance, identity, candidates);
                        registry
                    })
                    .await
                    else {
                        return;
                    };
                    registry = next;
                }
            }
            if let Err(error) = aow_i18n::spawn_blocking(move || {
                pending.extend(registry.poll());
                service.append(&pending)
            })
            .await
            .unwrap_or_else(|error| Err(error.into()))
            {
                tracing::error!(%error,"cannot persist final native completions");
            }
        })
    }
}
