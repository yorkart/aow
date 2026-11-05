use super::*;
use crate::host_connection::HostConnection;
use crate::settings::SettingsService;
use crate::settings::persistence::load_registry;
#[derive(Clone)]
pub(crate) struct ProjectService {
    pub(super) inner: Arc<ProjectState>,
    pub(super) settings: SettingsService,
    pub(super) node: HostConnection,
}
pub(super) struct ProjectState {
    pub(super) state_dir: Option<PathBuf>,
    pub(super) config: Option<aow_config::ConfigStore>,
    pub(super) projects_path: Option<PathBuf>,
    pub(super) state: Mutex<RegistryState>,
    pub(super) project_operation: Arc<tokio::sync::Mutex<()>>,
    pub(super) filesystem_operation: Arc<tokio::sync::RwLock<()>>,
    pub(super) removals: worktrees::removal::RemovalJobs,
    pub(super) creations: worktrees::creation::CreationJobs,
}
impl ProjectService {
    pub(crate) fn host(&self) -> &HostConnection {
        &self.node
    }
    pub(crate) fn new(
        state_dir: Option<PathBuf>,
        config: Option<aow_config::ConfigStore>,
        settings: SettingsService,
        node: HostConnection,
    ) -> Result<Self, ServiceError> {
        let projects_path = config
            .as_ref()
            .map(|c| c.directory().join("aow-projects.json"));
        let projects = projects_path
            .as_ref()
            .map(|p| load_registry(p))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            settings,
            node,
            inner: Arc::new(ProjectState {
                state_dir,
                config,
                projects_path,
                state: Mutex::new(RegistryState { projects }),
                project_operation: Arc::default(),
                filesystem_operation: Arc::default(),
                removals: worktrees::removal::RemovalJobs::in_memory(),
                creations: Default::default(),
            }),
        })
    }
    pub(super) fn lock(&self) -> Result<MutexGuard<'_, RegistryState>, ServiceError> {
        self.inner.state.lock().map_err(|_| ServiceError::Poisoned)
    }
    pub(crate) fn repository_roots(&self) -> Result<Vec<PathBuf>, ServiceError> {
        Ok(self
            .lock()?
            .projects
            .iter()
            .map(|p| PathBuf::from(&p.registered_path))
            .collect())
    }
    pub(crate) async fn filesystem_access(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.inner.filesystem_operation.read().await
    }
}

#[cfg(test)]
impl ProjectService {
    pub(crate) fn test_registry(&self) -> Result<MutexGuard<'_, RegistryState>, ServiceError> {
        self.lock()
    }
    pub(crate) fn test_registry_path(&self) -> Option<&Path> {
        self.inner.projects_path.as_deref()
    }
    pub(crate) fn test_config(&self) -> Option<&aow_config::ConfigStore> {
        self.inner.config.as_ref()
    }
}
