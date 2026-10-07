//! Managed Pi identification while an older terminald remains running.

use super::*;
use aow_protocol::TerminalAgentProcess;
use std::io::Read;

pub(super) async fn enrich(
    daemon: &TerminaldClient,
    tabs: &[TerminalTab],
    detected: &mut TerminalAgentList,
) {
    let candidates: Vec<_> = tabs
        .iter()
        .flat_map(|tab| &tab.panes)
        .filter(|pane| {
            pane.kind == TerminalPaneKind::Agent
                && pane.agent_id.as_deref() == Some("pi")
                && pane.status == TerminalPaneStatus::Running
                && detected.agents.contains_key(&pane.id)
                && detected.agents[&pane.id]
                    .as_deref()
                    .is_none_or(|agent| agent == "pi")
                && !detected.processes.contains_key(&pane.id)
        })
        .filter_map(|pane| {
            aow_agents::pi_bridge::binding_path(&pane.arguments, &pane.id)
                .map(|path| (pane.id.clone(), pane.cwd.clone(), path))
        })
        .collect();
    if candidates.is_empty() {
        return;
    }
    let Ok(health) = daemon.health().await else {
        return;
    };
    let Some(daemon_pid) = health.pid.and_then(|pid| i32::try_from(pid).ok()) else {
        return;
    };
    let mut running = Vec::new();
    for (id, cwd, path) in candidates {
        // Persisted pane state can lag a process exit. A stale binding must not
        // turn an exited runtime back into a live Agent during that interval.
        if let Ok(Some(runtime)) = daemon.get(&id).await
            && runtime.status == TerminalPaneStatus::Running
            && runtime.cwd == cwd
        {
            running.push((id, cwd, path));
        }
    }
    let Ok(processes) = tokio::task::spawn_blocking(move || {
        running
            .into_iter()
            .filter_map(|(id, cwd, path)| {
                identify(daemon_pid, &cwd, &path).map(|process| (id, process))
            })
            .collect::<Vec<_>>()
    })
    .await
    else {
        return;
    };
    for (id, (process, foreground)) in processes {
        detected.agents.insert(id.clone(), Some("pi".into()));
        if foreground {
            detected.processes.insert(id, process);
        }
    }
}

fn identify(
    daemon_pid: i32,
    cwd: &str,
    binding_path: &Path,
) -> Option<(TerminalAgentProcess, bool)> {
    #[derive(Deserialize)]
    struct Binding {
        pid: i32,
        cwd: PathBuf,
    }
    let file = std::fs::File::open(binding_path).ok()?;
    let binding: Binding = serde_json::from_reader(file.take(16 * 1024)).ok()?;
    let info = aow_process::info(binding.pid).ok()?;
    // Only the actual PTY child of this daemon can identify a managed pane.
    // Do not infer shell agents from titles, directories or a saved agent type.
    if info.parent != daemon_pid
        || info.session != info.pid
        || info.tty == 0
        || matches!(info.state, 'Z' | 'X' | 'T' | 't')
    {
        return None;
    }
    let live_cwd = aow_process::cwd(info.pid).ok()?;
    if live_cwd != binding.cwd.canonicalize().ok()?
        || live_cwd != Path::new(cwd).canonicalize().ok()?
    {
        return None;
    }
    let command = aow_process::command(info.pid);
    let arguments: Vec<_> = command
        .arguments
        .split(|b| *b == 0)
        .map(|arg| std::str::from_utf8(arg).unwrap_or(""))
        .collect();
    let agent = aow_agents::recognize_process(&aow_agents::ProcessInfo::new(
        command.executable.as_deref().and_then(Path::to_str),
        &arguments,
    ));
    if agent != Some(aow_agents::Agent::Pi)
        || !aow_process::same_process(info.pid, &info.start_time)
    {
        return None;
    }
    Some((
        TerminalAgentProcess {
            pid: info.pid,
            start_time: info.start_time,
            cwd: live_cwd.to_string_lossy().into_owned(),
            pi_binding: Some(binding_path.to_string_lossy().into_owned()),
        },
        info.group == info.foreground,
    ))
}

#[cfg(test)]
mod tests;
