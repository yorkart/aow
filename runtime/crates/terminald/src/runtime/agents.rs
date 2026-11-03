//! Read-only process detection shared by all terminal panes and browser clients.

use std::{collections::BTreeMap, time::Instant};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use aow_agents::process::{ProcessInfo, recognize_process};

use super::*;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use aow_protocol::{TerminalAgentProcess, TerminalPaneActivity};

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
        for runtime in &runtimes {
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
        let mut detected = TerminalAgentList {
            agents: cache.detected.agents.clone(),
            titles,
            processes: cache.detected.processes.clone(),
            activity: cache.detected.activity.clone(),
        };
        for runtime in runtimes {
            if detected
                .agents
                .get(&runtime.id)
                .and_then(|agent| agent.as_deref())
                == Some("pi")
                && let Some(process) = detected.processes.get_mut(&runtime.id)
            {
                process.pi_binding = runtime
                    .creation_spec
                    .environment
                    .get("AOW_PI_BINDING")
                    .filter(|path| !path.is_empty())
                    .cloned();
            }
        }
        Ok(detected)
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
            // Publish executable basenames only; never publish arguments or prompts.
            let command = aow_process::command(pid);
            let executable_name = command
                .executable
                .as_deref()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty());
            let argument_name = command
                .arguments
                .split(|byte| *byte == 0)
                .next()
                .and_then(|argument| std::str::from_utf8(argument).ok())
                .and_then(|argument| Path::new(argument).file_name())
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty());
            let command_name = executable_name.or(argument_name).map(str::to_owned);
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
            processes.insert(
                pid,
                Process {
                    info,
                    agent,
                    command_name,
                },
            );
        }
    }
    let mut detected = TerminalAgentList::default();
    for (id, session) in sessions {
        if let Some(root) = session.and_then(|session| processes.get(&session)) {
            detected.activity.insert(
                id.clone(),
                TerminalPaneActivity {
                    cwd: aow_process::cwd(root.info.pid)
                        .ok()
                        .map(|cwd| cwd.to_string_lossy().into_owned()),
                    foreground_command: foreground_command(root, &processes),
                },
            );
        }
        let selected = session.and_then(|session| select_process(session, &processes));
        detected.agents.insert(
            id.clone(),
            selected
                .and_then(|(process, _)| process.agent)
                .map(str::to_owned),
        );
        // A background process can supply the old agent badge, but must never
        // be treated as the conversation the user is interacting with.
        if let Some((process, true)) = selected
            && let Ok(cwd) = aow_process::cwd(process.info.pid)
        {
            detected.processes.insert(
                id.clone(),
                TerminalAgentProcess {
                    pid: process.info.pid,
                    start_time: process.info.start_time.clone(),
                    cwd: cwd.to_string_lossy().into_owned(),
                    pi_binding: None,
                },
            );
        }
    }
    Ok(detected)
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
#[derive(Debug)]
struct Process {
    info: aow_process::ProcessInfo,
    agent: Option<&'static str>,
    command_name: Option<String>,
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

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn foreground_command(root: &Process, processes: &BTreeMap<i32, Process>) -> Option<String> {
    let group = root.info.foreground;
    if group <= 1 || group == root.info.group {
        return None;
    }
    processes
        .values()
        .filter(|process| process.live_on(root) && process.info.group == group)
        .min_by_key(|process| (process.info.pid != group, process.info.pid))
        .and_then(|process| process.command_name.clone())
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
