//! Read-only process detection shared by all terminal panes and browser clients.

use std::{collections::BTreeMap, time::Instant};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use aow_agents::process::{ProcessInfo, recognize_process};

use super::*;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use aow_protocol::TerminalAgentProcess;

const CACHE_TTL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(super) struct AgentCache {
    captured: Option<Instant>,
    sessions: BTreeMap<String, Option<i32>>,
    detected: TerminalAgentList,
}

impl DaemonState {
    pub(super) fn agents(&self) -> Result<TerminalAgentList, TerminaldError> {
        let runtimes: Vec<_> = self.lock_runtimes()?.values().cloned().collect();
        let mut sessions = BTreeMap::new();
        let mut titles = BTreeMap::new();
        for runtime in runtimes {
            if runtime.is_deleted() {
                continue;
            }
            let running = runtime
                .metadata
                .lock()
                .map_err(|_| TerminaldError::Poisoned)?
                .status
                == TerminalPaneStatus::Running;
            sessions.insert(
                runtime.id.clone(),
                running.then_some(runtime.child_session_id).flatten(),
            );
            if running {
                let output = runtime
                    .output
                    .lock()
                    .map_err(|_| TerminaldError::Poisoned)?;
                if let Some(title) = &output.terminal.title {
                    titles.insert(runtime.id.clone(), title.clone());
                }
            }
        }
        let mut cache = self
            .inner
            .agent_cache
            .lock()
            .map_err(|_| TerminaldError::Poisoned)?;
        if cache.sessions != sessions
            || !cache
                .captured
                .is_some_and(|time| time.elapsed() < CACHE_TTL)
        {
            cache.detected = scan(&sessions)?;
            cache.sessions = sessions;
            cache.captured = Some(Instant::now());
        }
        Ok(TerminalAgentList {
            agents: cache.detected.agents.clone(),
            titles,
            processes: cache.detected.processes.clone(),
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn scan(_sessions: &BTreeMap<String, Option<i32>>) -> std::io::Result<TerminalAgentList> {
    Ok(TerminalAgentList::default())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn scan(sessions: &BTreeMap<String, Option<i32>>) -> std::io::Result<TerminalAgentList> {
    let wanted: std::collections::HashSet<_> = sessions.values().filter_map(|id| *id).collect();
    let mut processes = BTreeMap::new();
    if !wanted.is_empty() {
        for pid in aow_process::list_pids()? {
            let Ok(info) = aow_process::info(pid) else {
                continue;
            };
            if !wanted.contains(&info.session) {
                continue;
            }
            // Only entrypoints matter; never publish command lines or prompts.
            let command = aow_process::command(pid);
            let arguments: Vec<_> = command
                .arguments
                .split(|byte| *byte == 0)
                .map(|arg| std::str::from_utf8(arg).unwrap_or(""))
                .collect();
            let agent = recognize_process(&ProcessInfo::new(
                command.executable.as_deref().and_then(Path::to_str),
                &arguments,
            ))
            .map(|agent| agent.id());
            processes.insert(pid, Process { info, agent });
        }
    }
    let mut detected = TerminalAgentList::default();
    for (id, session) in sessions {
        let selected = session.and_then(|session| select_process(session, &processes));
        detected.agents.insert(
            id.clone(),
            selected
                .and_then(|(process, _)| process.agent)
                .map(str::to_owned),
        );
        // A background process can supply the old agent badge, but must never
        // be treated as the conversation the user is interacting with.
        if let Some((process, true)) = selected {
            if let Ok(cwd) = aow_process::cwd(process.info.pid) {
                detected.processes.insert(
                    id.clone(),
                    TerminalAgentProcess {
                        pid: process.info.pid,
                        start_time: process.info.start_time.clone(),
                        cwd: cwd.to_string_lossy().into_owned(),
                    },
                );
            }
        }
    }
    Ok(detected)
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
#[derive(Debug)]
struct Process {
    info: aow_process::ProcessInfo,
    agent: Option<&'static str>,
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
impl Process {
    fn live_on(&self, root: &Self) -> bool {
        let (process, root) = (&self.info, &root.info);
        process.session == root.session
            && process.tty == root.tty
            && root.tty != 0
            && !matches!(process.state, 'Z' | 'X' | 'T' | 't')
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn select_process(session: i32, processes: &BTreeMap<i32, Process>) -> Option<(&Process, bool)> {
    let root = processes.get(&session)?;
    let mut foreground_ancestors = std::collections::HashSet::new();
    for process in processes
        .values()
        .filter(|process| process.live_on(root) && process.info.group == root.info.foreground)
    {
        let mut current = process;
        for _ in 0..processes.len() {
            if !current.live_on(root) || !foreground_ancestors.insert(current.info.pid) {
                break;
            }
            let Some(parent) = processes.get(&current.info.parent) else {
                break;
            };
            current = parent;
        }
    }
    processes
        .values()
        .filter(|process| process.live_on(root) && process.agent.is_some())
        .min_by_key(|process| {
            (
                !foreground_ancestors.contains(&process.info.pid),
                process.info.pid,
            )
        })
        .map(|process| (process, foreground_ancestors.contains(&process.info.pid)))
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
