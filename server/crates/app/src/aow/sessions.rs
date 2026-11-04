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

#[cfg(test)]
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
            aow_i18n::text(
                "validation.removed_session_worktree_path_must_be_absolute_and_normalized",
            )
            .to_owned(),
        ));
    }
    Ok(path.to_path_buf())
}

#[derive(Debug, Clone)]
pub(crate) enum SessionReference {
    Node(aow_runtime_client::NativeSessionReference),
}
impl SessionReference {
    pub(crate) fn agent(&self) -> &str {
        match self {
            Self::Node(value) => &value.agent,
        }
    }
    pub(crate) fn session_id(&self) -> &str {
        match self {
            Self::Node(value) => &value.session_id,
        }
    }
    pub(crate) fn cwd(&self) -> &Path {
        match self {
            Self::Node(value) => &value.cwd,
        }
    }
    pub(crate) fn title(&self) -> &str {
        match self {
            Self::Node(value) => &value.title,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CachedSessionLocator {
    worktree: PathBuf,
    locator: SessionReference,
}

impl AowManager {
    pub(super) async fn registered_worktree_path(
        &self,
        worktree_path: &str,
    ) -> Result<PathBuf, AowError> {
        if let Some(runtime) = &self.inner.runtime_client {
            return runtime
                .resolve_native_worktree(Path::new(worktree_path), &self.repository_roots()?)
                .await
                .map_err(|error| AowError::Invalid(error.to_string()));
        }
        let requested = self.canonical_directory(Path::new(worktree_path)).await?;
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

    pub(crate) fn cache_native_sessions(
        &self,
        worktree: &Path,
        agent: Option<&str>,
        sessions: &[aow_runtime_client::NativeSession],
        replace: bool,
    ) -> Result<(), AowError> {
        let mut cache = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?;
        if replace {
            cache.retain(|_, entry| {
                entry.worktree != worktree
                    || agent.is_some_and(|agent| entry.locator.agent() != agent)
            });
        }
        for session in sessions {
            let reference = session.reference.clone();
            if !reference.cwd.starts_with(worktree) {
                continue;
            }
            cache.insert(
                session_locator_key(worktree, &reference.agent, &reference.session_id),
                CachedSessionLocator {
                    worktree: worktree.into(),
                    locator: SessionReference::Node(reference),
                },
            );
        }
        Ok(())
    }
    pub(super) fn session_locator(
        &self,
        worktree: &Path,
        agent: &str,
        session_id: &str,
    ) -> Result<SessionReference, AowError> {
        let locator = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .get(&session_locator_key(worktree, agent, session_id))
            .filter(|cached| {
                cached.worktree == worktree && cached.locator.cwd().starts_with(worktree)
            })
            .map(|cached| cached.locator.clone())
            .ok_or_else(|| {
                AowError::Invalid(
                    aow_i18n::text("validation.agent_session_is_not_in_the_scanned_worktree")
                        .to_owned(),
                )
            })?;
        Ok(locator)
    }
}

fn session_locator_key(worktree: &Path, agent: &str, session_id: &str) -> String {
    format!("{}\0{agent}\0{session_id}", worktree.display())
}

pub(crate) fn public_native_sessions<T: serde::Serialize>(
    sessions: T,
) -> serde_json::Result<serde_json::Value> {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            serde_json::Value::Object(object) => {
                object.remove("reference");
                object.remove("current");
                if let Some(items) = object.get_mut("sessions") {
                    strip(items);
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(sessions)?;
    strip(&mut value);
    Ok(value)
}
