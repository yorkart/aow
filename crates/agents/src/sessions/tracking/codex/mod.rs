use super::*;
use crate::sessions::codex_like::CodexLikeSessionProvider;
use std::{collections::BTreeSet, path::PathBuf};

mod files;
#[cfg(test)]
mod tests;

pub(super) struct Codex;

impl Codex {
    pub(super) fn try_candidate_sessions(
        &self,
        target: &SessionTarget,
        cwd: &Path,
        roots: SessionRoots,
    ) -> Option<Vec<AgentSessionLocator>> {
        if let SessionTarget::Id(id) = target {
            let store = CodexLikeSessionProvider {
                definition: &crate::CODEX,
                home: roots.codex,
            };
            return Some(
                store
                    .find_sessions([id.as_str()])
                    .ok()?
                    .into_iter()
                    .filter(|session| {
                        files::canonical(&session.cwd_path()) == files::canonical(cwd)
                    })
                    .map(|session| session.locator())
                    .collect(),
            );
        }
        Some(codex_like::candidates(Agent::Codex, target, cwd, roots))
    }
}

impl AgentSessionTracker for Codex {
    async fn resolve_live_session(&self, context: LiveSessionContext<'_>) -> SessionResolution {
        let fallback = codex_like::resolve(LiveSessionContext { ..context });
        if !context
            .environment
            .contains_key(crate::codex_command::TERMINAL_ENV)
        {
            return fallback;
        }
        let Some(pid) = context.pid else {
            return fallback;
        };
        let Some(home) = context.environment.get("HOME") else {
            return SessionResolution::Unavailable;
        };
        let roots = SessionRoots::from_configuration(home, context.environment);
        let cwd = PathBuf::from(context.cwd);
        let title = codex_like::normalized_title(context.title, context.cwd);
        tokio::task::spawn_blocking(move || resolve(pid, &roots.codex, &cwd, &title, fallback))
            .await
            .unwrap_or(SessionResolution::Unavailable)
    }

    fn candidate_sessions(
        &self,
        target: &SessionTarget,
        cwd: &Path,
        roots: SessionRoots,
    ) -> Vec<AgentSessionLocator> {
        self.try_candidate_sessions(target, cwd, roots)
            .unwrap_or_default()
    }

    fn task_stop_parser(&self) -> Box<dyn TaskStopParser> {
        Box::new(codex_like::Parser::default())
    }
}

fn isolated_tui(command: &aow_process::ProcessCommand) -> bool {
    if command.arguments.len() >= 16 * 1024 || command.arguments.last() != Some(&0) {
        return false;
    }
    let Ok(arguments) = command.arguments[..command.arguments.len() - 1]
        .split(|byte| *byte == 0)
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    let executable = command.executable.as_deref().and_then(Path::to_str);
    let process = crate::ProcessInfo::new(executable, &arguments);
    if crate::recognize_process(&process) != Some(Agent::Codex) || process.script.is_some() {
        return false;
    }
    let flags = crate::codex_command::flags(&arguments[1..]);
    flags.interactive && flags.no_daemon.is_some() && !flags.remote
}

fn resolve(
    pid: i32,
    root: &Path,
    cwd: &Path,
    title: &str,
    fallback: SessionResolution,
) -> SessionResolution {
    let Ok(process) = aow_process::info(pid) else {
        return SessionResolution::Unavailable;
    };
    let command = aow_process::command(pid);
    if !isolated_tui(&command) {
        return fallback;
    }
    let result = match aow_process::open_files(pid, &process.start_time) {
        Ok(open) => resolve_files(root, cwd, title, &open),
        // Native inspection may be unavailable for this OS or permission boundary.
        Err(_) => fallback,
    };
    if aow_process::same_process(pid, &process.start_time) {
        result
    } else {
        SessionResolution::Unavailable
    }
}

fn resolve_files(
    root: &Path,
    cwd: &Path,
    title: &str,
    open: &[aow_process::OpenFile],
) -> SessionResolution {
    let ids = files::thread_ids(root, open);
    if ids.is_empty() || ids.len() > 128 {
        return SessionResolution::NotFound;
    }
    let store = CodexLikeSessionProvider {
        definition: &crate::CODEX,
        home: root.into(),
    };
    let Ok(sessions) = store.find_sessions(ids.iter().map(String::as_str)) else {
        return SessionResolution::Unavailable;
    };
    select(&ids, &sessions, cwd, title)
}

fn select(
    ids: &BTreeSet<String>,
    sessions: &[crate::sessions::AgentSession],
    cwd: &Path,
    title: &str,
) -> SessionResolution {
    // A fresh /new thread can own a lock before it has a database row. Never
    // discard it and accidentally choose the old, still-open rollout instead.
    if sessions.len() != ids.len() {
        return SessionResolution::NotFound;
    }
    let cwd = files::canonical(cwd);
    let candidates: Vec<_> = sessions
        .iter()
        .filter(|session| {
            files::canonical(&session.cwd_path()) == cwd
                && (if title.is_empty() {
                    ids.len() == 1
                } else {
                    codex_like::title_matches(title, &session.title)
                })
        })
        .collect();
    match candidates.as_slice() {
        [session] => SessionResolution::Resolved(SessionTarget::Id(session.session_id.clone())),
        _ => SessionResolution::NotFound,
    }
}
