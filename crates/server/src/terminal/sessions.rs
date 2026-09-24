//! On-demand, read-only association of a terminal pane with native agent history.

use super::*;
use aow_agents::sessions::{
    AgentSession, SessionEnvironment, SessionRoots,
    tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget},
};
use aow_protocol::TerminalAgentProcess;

#[derive(Serialize)]
pub(super) struct PaneSessions {
    agent: String,
    cwd: String,
    title: String,
    process: Option<TerminalAgentProcess>,
    /// The native session identity resolved by the agent adapter, if available.
    live_session_id: Option<String>,
    sessions: Vec<AgentSession>,
}

pub(super) async fn list(
    State(state): State<AppState>,
    AxumPath((tab_id, pane_id)): AxumPath<(String, String)>,
) -> Result<Json<PaneSessions>, HttpError> {
    let tab = state
        .terminals
        .get_snapshot(&tab_id)
        .map_err(terminal_http_error)?;
    let pane = tab
        .panes
        .iter()
        .find(|pane| pane.id == pane_id)
        .ok_or_else(|| terminal_http_error(TerminalError::PaneNotFound(pane_id.clone())))?;
    let detected = state
        .terminals
        .agents(Some(&tab.workspace_root))
        .await
        .map_err(terminal_http_error)?;
    let agent = detected
        .agents
        .get(&pane_id)
        .cloned()
        .unwrap_or_else(|| pane.agent_id.clone())
        .filter(|_| pane.status == TerminalPaneStatus::Running)
        .filter(|id| {
            aow_agents::Agent::from_id(id)
                .and_then(aow_agents::Agent::sessions)
                .is_some()
        })
        .ok_or_else(|| {
            terminal_http_error(TerminalError::Conflict(
                "当前面板没有可读取会话的 Agent".into(),
            ))
        })?;
    let process = detected.processes.get(&pane_id).cloned();
    let cwd = process
        .as_ref()
        .map_or_else(|| pane.cwd.clone(), |process| process.cwd.clone());
    let title = detected.titles.get(&pane_id).cloned().unwrap_or_default();
    let environment = process.as_ref().and_then(process_environment);
    // A process may exit between the daemon scan and this request. Never query
    // another process after PID reuse or use the server's config for that PID.
    if process.is_some() && environment.is_none() {
        return Err(terminal_http_error(TerminalError::Conflict(
            "Agent 进程已变化，请刷新会话列表".into(),
        )));
    }
    let roots = environment.as_ref().map_or_else(
        || SessionRoots::from_environment(&crate::PROCESS_HOME),
        |environment| {
            SessionRoots::from_configuration(
                environment
                    .get("HOME")
                    .map_or(crate::PROCESS_HOME.as_path(), PathBuf::as_path),
                environment,
            )
        },
    );
    let tracker = aow_agents::Agent::from_id(&agent).and_then(aow_agents::Agent::session_tracking);
    let live_session_id = if let (Some(tracker), Some(process), Some(environment)) =
        (tracker, &process, &environment)
    {
        match tracker
            .resolve_live_session(LiveSessionContext {
                pid: Some(process.pid),
                cwd: &cwd,
                title: &title,
                environment,
            })
            .await
        {
            SessionResolution::Resolved(SessionTarget::Id(id)) => Some(id),
            _ => None,
        }
    } else {
        None
    };
    let scan_cwd = PathBuf::from(&cwd);
    let scan_agent = agent.clone();
    let exact_id = live_session_id.clone();
    let sessions = tokio::task::spawn_blocking(move || {
        let mut sessions =
            aow_agents::sessions::list_sessions(&scan_cwd, Some(&scan_agent), roots.clone());
        if let Some(id) = exact_id {
            if !sessions
                .iter()
                .any(|session| session.locator().session_id == id)
            {
                if let Some(session) = aow_agents::sessions::find_session(&scan_agent, &id, roots) {
                    sessions.insert(0, session);
                }
            }
        }
        sessions
    })
    .await
    .map_err(|error| HttpError::internal(format!("agent session scan failed: {error}")))?;
    if process
        .as_ref()
        .is_some_and(|process| !same_process(process))
    {
        return Err(terminal_http_error(TerminalError::Conflict(
            "Agent 进程已变化，请刷新会话列表".into(),
        )));
    }
    // The pane supplies the trusted scan scope, including directories opened
    // with `cd` that aren't registered project worktrees. Snapshot reads still
    // go through the existing provider's transcript-root validation.
    for session in &sessions {
        let cwd = tokio::fs::canonicalize(session.cwd_path())
            .await
            .unwrap_or_else(|_| session.cwd_path());
        state
            .aow
            .cache_session_locator(&cwd, session)
            .map_err(crate::aow::aow_http_error)?;
    }
    Ok(Json(PaneSessions {
        agent,
        cwd,
        title,
        process,
        live_session_id,
        sessions,
    }))
}

pub(super) fn same_process(process: &TerminalAgentProcess) -> bool {
    aow_process::same_process(process.pid, &process.start_time)
}

pub(super) fn process_environment(process: &TerminalAgentProcess) -> Option<SessionEnvironment> {
    let bytes = aow_process::environment(process.pid, &process.start_time).ok()?;
    Some(configuration_environment(&bytes))
}

fn configuration_environment(bytes: &[u8]) -> SessionEnvironment {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let (key, value) = std::str::from_utf8(entry).ok()?.split_once('=')?;
            (matches!(key, "HOME" | "PATH")
                || aow_agents::KNOWN_AGENTS
                    .iter()
                    .any(|agent| agent.definition().configuration_env.contains(&key)))
            .then(|| (key.to_owned(), PathBuf::from(value)))
            .filter(|(_, value)| !value.as_os_str().is_empty())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_configuration_paths_are_read_from_the_agent_environment() {
        let keys: HashSet<_> = aow_agents::KNOWN_AGENTS
            .iter()
            .flat_map(|agent| agent.definition().configuration_env.iter().copied())
            .collect();
        let mut bytes = b"HOME=/home/user\0PATH=/usr/bin\0SECRET=hidden\0".to_vec();
        for key in &keys {
            bytes.extend_from_slice(format!("{key}=/config/{key}\0").as_bytes());
        }
        let environment = configuration_environment(&bytes);
        assert_eq!(environment.len(), keys.len() + 2);
        for key in keys {
            assert_eq!(environment[key], PathBuf::from(format!("/config/{key}")));
            assert!(configuration_environment(format!("{key}=\0").as_bytes()).is_empty());
        }
        assert!(!environment.contains_key("SECRET"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_process_environment_uses_the_agents_config_and_rejects_stale_identity() {
        use std::process::{Command, Stdio};
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let ready = root.join("environment.ready");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "terminal::sessions::tests::native_environment_fixture",
        ]);
        command.env("AOW_NATIVE_ENV_FIXTURE", "1");
        command.env("AOW_NATIVE_ENV_READY", &ready);
        let keys: Vec<_> = aow_agents::KNOWN_AGENTS
            .iter()
            .flat_map(|agent| agent.definition().configuration_env.iter().copied())
            .chain(["HOME", "PATH"])
            .collect();
        for key in &keys {
            command.env(key, root.join(format!("{key} with spaces")));
        }
        let mut child = ChildGuard(
            command
                .env("AOW_TEST_SECRET", "must not be retained")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        // A PID can be observable before the exec has exposed the fixture's
        // environment through /proc. Wait for the child to enter the fixture.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "native environment fixture exited before becoming ready"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "native environment fixture did not become ready"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let info = aow_process::info(child.0.id() as i32).unwrap();
        let mut process = TerminalAgentProcess {
            pid: info.pid,
            start_time: info.start_time,
            cwd: root.to_string_lossy().into_owned(),
        };
        assert!(same_process(&process));
        let environment = process_environment(&process).unwrap();
        assert_eq!(environment.len(), keys.len());
        for key in keys {
            assert_eq!(environment[key], root.join(format!("{key} with spaces")));
        }
        assert!(!environment.contains_key("AOW_TEST_SECRET"));
        let original = process.start_time.clone();
        process.start_time = "different start identity".into();
        assert!(process_environment(&process).is_none());
        process.start_time = original;
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(!same_process(&process));
        assert!(process_environment(&process).is_none());
    }

    #[test]
    #[ignore = "subprocess fixture invoked by native process/session tests"]
    fn native_environment_fixture() {
        if std::env::var_os("AOW_NATIVE_ENV_FIXTURE").is_some() {
            if let Some(ready) = std::env::var_os("AOW_NATIVE_ENV_READY") {
                std::fs::write(ready, b"ready").unwrap();
            }
            std::io::stdin().read_line(&mut String::new()).unwrap();
        }
    }
}
