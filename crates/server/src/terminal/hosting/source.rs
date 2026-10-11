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
        .context("当前实例没有运行中的 Agent")?;
    let process = detected
        .processes
        .get(pane_id)
        .cloned()
        .context("无法确认 Agent 进程身份，请升级终端服务")?;
    let environment = sessions::process_environment(&process).context("Agent 进程已变化")?;
    let tracker = aow_agents::Agent::from_id(agent)
        .and_then(aow_agents::Agent::session_tracking)
        .context("该 Agent 暂不支持托管")?;
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
        _ => anyhow::bail!("尚未定位 Agent 当前会话，请在会话开始后重试"),
    };
    let roots = SessionRoots::from_configuration(
        environment
            .get("HOME")
            .map_or(crate::PROCESS_HOME.as_path(), PathBuf::as_path),
        &environment,
    );
    let cwd = PathBuf::from(&process.cwd);
    let locator = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut candidates = tracker.candidate_sessions(&target, &cwd, roots);
        ensure!(
            candidates.len() == 1,
            "无法唯一定位当前 Agent 会话，请为会话设置唯一标题后重试"
        );
        Ok(candidates.remove(0))
    })
    .await??;
    ensure!(sessions::same_process(&process), "Agent 进程已变化");
    Ok(Source { locator, process })
}

pub(super) fn matches(hosting: &TerminalHosting, source: &Source) -> bool {
    hosting.agent == source.locator.agent
        && hosting.session_id == source.locator.session_id
        && hosting.process == source.process
}
