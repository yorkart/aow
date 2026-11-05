use super::AgentService;
use crate::error::ServiceError;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) fn removed_directory_path(path: &Path) -> Result<PathBuf, ServiceError> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(ServiceError::Invalid(
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

impl AgentService {
    pub(crate) async fn resolve_session_locator(
        &self,
        session_id: &str,
        agent: &str,
        worktree_path: &str,
    ) -> Result<SessionReference, ServiceError> {
        if agent.is_empty() || session_id.is_empty() || session_id.len() > 256 {
            return Err(ServiceError::Invalid(
                aow_i18n::text("validation.invalid_agent_session_identity").to_owned(),
            ));
        }
        // A locator is only cached by a registered-session scan or a trusted
        // automation run lookup. Once one exists, the exact workspace key is
        // enough to read its snapshot, including an automation-created worktree
        // that is intentionally not registered as a project worktree.
        let requested = Path::new(worktree_path);
        {
            let runtime = self.node.agent_node().await?;
            if let Ok(reference) = self.session_locator(requested, agent, session_id) {
                return Ok(reference);
            }
            let cwd = runtime
                .resolve_directory(requested)
                .await
                .map_err(|error| ServiceError::Invalid(error.to_string()))?;
            self.session_locator(&cwd, agent, session_id)
        }
    }

    pub(crate) fn cache_native_sessions(
        &self,
        worktree: &Path,
        agent: Option<&str>,
        sessions: &[aow_runtime_client::NativeSession],
        replace: bool,
    ) -> Result<(), ServiceError> {
        let mut cache = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| ServiceError::Poisoned)?;
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
    pub(crate) fn session_locator(
        &self,
        worktree: &Path,
        agent: &str,
        session_id: &str,
    ) -> Result<SessionReference, ServiceError> {
        let locator = self
            .inner
            .session_locators
            .lock()
            .map_err(|_| ServiceError::Poisoned)?
            .get(&session_locator_key(worktree, agent, session_id))
            .filter(|cached| {
                cached.worktree == worktree && cached.locator.cwd().starts_with(worktree)
            })
            .map(|cached| cached.locator.clone())
            .ok_or_else(|| {
                ServiceError::Invalid(
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
