use super::*;
use anyhow::{Context, Result, ensure};
use aow_agents::sessions::{
    AgentSessionLocator, SessionRoots,
    tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution},
};
use aow_protocol::TerminalAgentProcess;

pub(super) struct Source {
    pub locator: AgentSessionLocator,
    pub process: TerminalAgentProcess,
}

pub(super) async fn read(manager: &TerminalManager, pane_id: &str) -> Result<Source> {
    let detected = manager.agents(None).await?;
    let agent = detected
        .agents
        .get(pane_id)
        .and_then(|agent| agent.as_deref())
        .context(aow_i18n::text(
            "server.no_running_agent_in_the_current_instance",
        ))?;
    let process = detected
        .processes
        .get(pane_id)
        .cloned()
        .context(aow_i18n::text(
            "server.unable_to_verify_agent_process_identity_upgrade_the_terminal",
        ))?;
    let environment = sessions::process_environment(&process)
        .context(aow_i18n::text("server.agent_process_changed"))?;
    let tracker = aow_agents::Agent::from_id(agent)
        .and_then(aow_agents::Agent::session_tracking)
        .context(aow_i18n::text(
            "server.this_agent_does_not_support_autopilot_yet",
        ))?;
    let target = match tracker
        .resolve_live_session_with_timeout(
            LiveSessionContext {
                pid: Some(process.pid),
                cwd: &process.cwd,
                title: detected.titles.get(pane_id).map_or("", String::as_str),
                environment: &environment,
            },
            manager.inner.session_query_timeout,
        )
        .await
    {
        SessionResolution::Resolved(target) => target,
        _ => anyhow::bail!(aow_i18n::message(
            "server.current_agent_session_not_identified_yet_retry_after_the",
            &[]
        )),
    };
    let roots = SessionRoots::from_configuration(
        environment
            .get("HOME")
            .map_or(crate::PROCESS_HOME.as_path(), PathBuf::as_path),
        &environment,
    );
    let cwd = PathBuf::from(&process.cwd);
    let locator = aow_i18n::spawn_blocking(move || -> Result<_> {
        let mut candidates = tracker.candidate_sessions(&target, &cwd, roots);
        ensure!(
            candidates.len() == 1,
            aow_i18n::message(
                "server.unable_to_uniquely_identify_the_current_agent_session_set",
                &[]
            )
        );
        Ok(candidates.remove(0))
    })
    .await??;
    ensure!(
        sessions::same_process(&process),
        aow_i18n::message("server.agent_process_changed", &[])
    );
    Ok(Source { locator, process })
}

pub(super) fn matches(hosting: &TerminalHosting, source: &Source) -> bool {
    hosting.agent == source.locator.agent
        && hosting.session_id == source.locator.session_id
        && hosting.process == source.process
}
