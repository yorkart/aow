use super::*;
use crate::{
    Agent,
    sessions::{
        process::{process_environment, same_process},
        tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget},
    },
};
use aow_protocol::TerminalAgentProcess;
use aow_runtime_contract::agents::{
    NativePaneRequest as PaneRequest, NativeProcessCheck as ProcessCheck,
    ResolveSessionWorktree as Worktree,
};

pub(crate) struct Binding {
    pub agent: &'static str,
    pub process: TerminalAgentProcess,
    pub cwd: String,
    pub title: String,
    pub environment: SessionEnvironment,
    pub roots: SessionRoots,
    pub target: Option<SessionTarget>,
}
impl SessionCatalog {
    pub(crate) fn observer(&self) -> &dyn ManagedTerminals {
        self.inner.observer.as_ref()
    }
    pub(crate) async fn binding(&self, pane_id: &str) -> Result<Option<Binding>> {
        let detected = self.inner.observer.agents().await?;
        let Some(agent) = detected
            .agents
            .get(pane_id)
            .and_then(|agent| agent.as_deref())
            .and_then(Agent::from_id)
        else {
            return Ok(None);
        };
        let Some(tracker) = agent.session_tracking() else {
            return Ok(None);
        };
        let Some(process) = detected.processes.get(pane_id).cloned() else {
            return Ok(None);
        };
        let identity = process.clone();
        let environment = tokio::task::spawn_blocking(move || process_environment(&identity))
            .await?
            .context("Agent process changed")?;
        let cwd = process.cwd.clone();
        let title = detected.titles.get(pane_id).cloned().unwrap_or_default();
        let target = match tracker
            .resolve_live_session_with_timeout(
                LiveSessionContext {
                    pid: Some(process.pid),
                    cwd: &cwd,
                    title: &title,
                    environment: &environment,
                },
                crate::sessions::tracking::DEFAULT_QUERY_TIMEOUT,
            )
            .await
        {
            SessionResolution::Resolved(target) => Some(target),
            SessionResolution::NotFound => None,
            SessionResolution::Unavailable => {
                anyhow::bail!("Native session temporarily unavailable")
            }
        };
        let home = environment
            .get("HOME")
            .cloned()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .context("Node HOME is unavailable")?;
        let roots = SessionRoots::from_configuration(&home, &environment);
        ensure!(same_process(&process), "Agent process changed");
        Ok(Some(Binding {
            agent: agent.id(),
            process,
            cwd,
            title,
            environment,
            roots,
            target,
        }))
    }
}
pub async fn read(service: SessionCatalog, request: PaneRequest) -> SessionResult {
    let binding = service
        .binding(&request.pane_id)
        .await
        .map_err(|e| error(SessionErrorKind::Conflict, e))?
        .ok_or_else(|| error(SessionErrorKind::Conflict, "No native session in this pane"))?;
    tokio::task::spawn_blocking(move || {
        let tracker = Agent::from_id(binding.agent)
            .and_then(Agent::session_tracking)
            .unwrap();
        let candidates = binding
            .target
            .as_ref()
            .map(|target| {
                tracker.candidate_sessions(
                    target,
                    std::path::Path::new(&binding.cwd),
                    binding.roots.clone(),
                )
            })
            .unwrap_or_default();
        let current = if candidates.len() == 1 {
            let session = crate::sessions::AgentSession::new(
                candidates[0].clone(),
                chrono::Utc::now(),
                chrono::Utc::now(),
            );
            let metadata = service
                .remember(&session)
                .map_err(|e| error(SessionErrorKind::Internal, e))?;
            Some(
                serde_json::from_value::<SessionReference>(metadata["reference"].clone())
                    .map_err(|e| error(SessionErrorKind::Internal, e))?,
            )
        } else {
            None
        };
        let live_session_id = match &binding.target {
            Some(SessionTarget::Id(id)) => Some(id.clone()),
            _ => None,
        };
        let mut sessions = crate::sessions::list_sessions(
            std::path::Path::new(&binding.cwd),
            Some(binding.agent),
            binding.roots,
        );
        if let Some(reference) = &current
            && !sessions
                .iter()
                .any(|session| session.locator().session_id == reference.session_id)
        {
            let locator = service
                .locate(reference)
                .map_err(|e| error(SessionErrorKind::Internal, e))?;
            sessions.insert(
                0,
                crate::sessions::AgentSession::new(locator, chrono::Utc::now(), chrono::Utc::now()),
            );
        }
        let values: Result<Vec<_>> = sessions
            .iter()
            .map(|session| service.remember(session))
            .collect();
        ensure_current(&binding.process)?;
        Ok(serde_json::json!({
            "agent":binding.agent,"cwd":binding.cwd,"title":binding.title,"process":binding.process,
            "live_session_id":live_session_id,"current":current,
            "sessions":values.map_err(|e|error(SessionErrorKind::Internal,e))?
        }))
    })
    .await
    .map_err(|e| error(SessionErrorKind::Internal, e))?
}

fn ensure_current(process: &TerminalAgentProcess) -> Result<(), SessionError> {
    if !same_process(process) {
        return Err(error(SessionErrorKind::Conflict, "Agent process changed"));
    }
    Ok(())
}
pub async fn process_check(service: SessionCatalog, request: ProcessCheck) -> SessionResult {
    let detected = service
        .inner
        .observer
        .agents()
        .await
        .map_err(|e| error(SessionErrorKind::Internal, e))?;
    let matches = detected
        .processes
        .get(&request.pane_id)
        .is_some_and(|process| {
            process.pid == request.process.pid
                && process.start_time == request.process.start_time
                && process.cwd == request.process.cwd
                && same_process(process)
        });
    Ok(serde_json::json!({"current":matches}))
}
pub async fn resolve_worktree(request: Worktree) -> SessionResult {
    let cwd = aow_workspaces::resolve_registered_worktree(&request.cwd, &request.registered_roots)
        .await
        .map_err(|error| super::error(SessionErrorKind::Invalid, error))?;
    Ok(serde_json::json!({"path":cwd}))
}
