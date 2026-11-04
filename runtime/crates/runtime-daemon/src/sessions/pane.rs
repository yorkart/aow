use super::*;
use aow_agents::{
    Agent,
    sessions::{
        process::{process_environment, same_process},
        tracking::{AgentSessionTracker, LiveSessionContext, SessionResolution, SessionTarget},
    },
};
use aow_protocol::TerminalAgentProcess;

pub(crate) struct Binding {
    pub agent: &'static str,
    pub process: TerminalAgentProcess,
    pub cwd: String,
    pub title: String,
    pub environment: SessionEnvironment,
    pub roots: SessionRoots,
    pub target: Option<SessionTarget>,
}
impl SessionService {
    pub(crate) fn observer(&self) -> &TerminalObserver {
        &self.inner.observer
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
                aow_agents::sessions::tracking::DEFAULT_QUERY_TIMEOUT,
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
#[derive(Deserialize)]
pub(super) struct PaneRequest {
    pane_id: String,
}
pub(super) async fn read(
    State(service): State<SessionService>,
    Json(request): Json<PaneRequest>,
) -> Response {
    let binding = service
        .binding(&request.pane_id)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, e))?
        .ok_or_else(|| error(StatusCode::CONFLICT, "No native session in this pane"))?;
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
            let session = aow_agents::sessions::AgentSession::new(
                candidates[0].clone(),
                chrono::Utc::now(),
                chrono::Utc::now(),
            );
            let metadata = service
                .remember(&session)
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            Some(
                serde_json::from_value::<SessionReference>(metadata["reference"].clone())
                    .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?,
            )
        } else {
            None
        };
        let live_session_id = match &binding.target {
            Some(SessionTarget::Id(id)) => Some(id.clone()),
            _ => None,
        };
        let mut sessions = aow_agents::sessions::list_sessions(
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
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
            sessions.insert(
                0,
                aow_agents::sessions::AgentSession::new(
                    locator,
                    chrono::Utc::now(),
                    chrono::Utc::now(),
                ),
            );
        }
        let values: Result<Vec<_>> = sessions
            .iter()
            .map(|session| service.remember(session))
            .collect();
        ensure_current(&binding.process)?;
        let sessions = values
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<aow_runtime_contract::NativeSessionRecord>, _>>()
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Json(serde_json::json!(
            aow_runtime_contract::NativePaneSessionRecord {
                agent: binding.agent.into(),
                cwd: binding.cwd,
                title: binding.title,
                process: Some(binding.process),
                live_session_id,
                current,
                sessions,
            }
        )))
    })
    .await
    .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
}

fn ensure_current(
    process: &TerminalAgentProcess,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !same_process(process) {
        return Err(error(StatusCode::CONFLICT, "Agent process changed"));
    }
    Ok(())
}
#[derive(Deserialize)]
pub(super) struct ProcessCheck {
    pane_id: String,
    process: TerminalAgentProcess,
}
pub(super) async fn process_check(
    State(service): State<SessionService>,
    Json(request): Json<ProcessCheck>,
) -> Response {
    let detected = service
        .inner
        .observer
        .agents()
        .await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let matches = detected
        .processes
        .get(&request.pane_id)
        .is_some_and(|process| {
            process.pid == request.process.pid
                && process.start_time == request.process.start_time
                && process.cwd == request.process.cwd
                && same_process(process)
        });
    Ok(Json(serde_json::json!({"current":matches})))
}
#[derive(Deserialize)]
pub(super) struct Worktree {
    cwd: PathBuf,
    registered_roots: Vec<PathBuf>,
}
async fn git(cwd: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(output.status.success(), "Node Git lookup failed");
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
pub(super) async fn resolve_worktree(Json(request): Json<Worktree>) -> Response {
    async {
        ensure!(
            request.cwd.is_absolute(),
            "Workspace path must be absolute on the node"
        );
        let cwd = tokio::fs::canonicalize(&request.cwd).await?;
        let top =
            tokio::fs::canonicalize(git(&cwd, &["rev-parse", "--show-toplevel"]).await?).await?;
        ensure!(cwd == top, "Workspace path must be a worktree root");
        let common = tokio::fs::canonicalize(
            git(
                &cwd,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await?,
        )
        .await?;
        for root in request.registered_roots {
            if !root.is_absolute() {
                continue;
            }
            if let Ok(value) = git(
                &root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .await
                && tokio::fs::canonicalize(value)
                    .await
                    .is_ok_and(|candidate| candidate == common)
            {
                return Ok(Json(serde_json::json!({"path":cwd})));
            }
        }
        anyhow::bail!("Workspace is not owned by a registered project")
    }
    .await
    .map_err(|e: anyhow::Error| error(StatusCode::BAD_REQUEST, e))
}
