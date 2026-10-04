use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct AgentSessionsQuery {
    pub(super) worktree_path: String,
    pub(super) agent: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentSessionSnapshotQuery {
    pub(super) agent: String,
    pub(super) worktree_path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct AutomationRunSessionQuery {
    pub(super) task_id: String,
    pub(super) run_id: String,
}

pub(super) fn removed_directory_path(path: &Path) -> Result<PathBuf, AowError> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(AowError::Invalid(
            "removed session worktree path must be absolute and normalized".to_owned(),
        ));
    }
    Ok(path.to_path_buf())
}

#[derive(Debug, Clone)]
pub(super) struct CachedSessionLocator {
    worktree: PathBuf,
    locator: aow_agents::sessions::AgentSessionLocator,
}

impl AowManager {
    pub(super) async fn registered_worktree_path(
        &self,
        worktree_path: &str,
    ) -> Result<PathBuf, AowError> {
        let requested = paths::canonical_directory(Path::new(worktree_path)).await?;
        let projects = self.projects().await?;
        let registered = projects.iter().any(|project| {
            project.error.is_none()
                && project
                    .worktrees
                    .iter()
                    .any(|worktree| Path::new(&worktree.path) == requested)
        });
        if !registered {
            return Err(AowError::Invalid(format!(
                "session cwd is not a registered worktree: {}",
                requested.display()
            )));
        }
        Ok(requested)
    }

    pub(super) fn replace_session_locators_for_agent(
        &self,
        worktree: &Path,
        agent: Option<&str>,
        sessions: &[aow_agents::sessions::AgentSession],
    ) -> Result<(), AowError> {
        let mut locators = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?;
        locators.retain(|_, locator| {
            locator.worktree != worktree
                || agent.is_some_and(|agent| locator.locator.agent != agent)
        });
        for session in sessions {
            let locator = session.locator();
            locators.insert(
                session_locator_key(worktree, locator.agent, &locator.session_id),
                CachedSessionLocator {
                    worktree: worktree.to_path_buf(),
                    locator,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn cache_session_locator(
        &self,
        worktree: &Path,
        session: &aow_agents::sessions::AgentSession,
    ) -> Result<(), AowError> {
        let mut locator = session.locator();
        locator.cwd = std::fs::canonicalize(&locator.cwd).unwrap_or(locator.cwd);
        self.inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .insert(
                session_locator_key(worktree, locator.agent, &locator.session_id),
                CachedSessionLocator {
                    worktree: worktree.to_path_buf(),
                    locator,
                },
            );
        Ok(())
    }

    pub(super) fn session_locator(
        &self,
        worktree: &Path,
        agent: &str,
        session_id: &str,
    ) -> Result<aow_agents::sessions::AgentSessionLocator, AowError> {
        let locator = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .get(&session_locator_key(worktree, agent, session_id))
            .filter(|cached| {
                cached.worktree == worktree && cached.locator.cwd.starts_with(worktree)
            })
            .map(|cached| cached.locator.clone())
            .ok_or_else(|| {
                AowError::Invalid("agent session is not in the scanned worktree".to_owned())
            })?;
        Ok(locator)
    }
}

fn session_locator_key(worktree: &Path, agent: &str, session_id: &str) -> String {
    format!("{}\0{agent}\0{session_id}", worktree.display())
}
