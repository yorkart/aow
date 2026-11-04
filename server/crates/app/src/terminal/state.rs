use super::*;

#[derive(Clone)]
pub(crate) struct TerminalManager {
    pub(super) inner: Arc<ManagerInner>,
}

pub(super) struct ManagerInner {
    #[cfg(test)]
    pub(super) node_workspace: std::sync::atomic::AtomicBool,
    pub(super) metadata_path: Option<PathBuf>,
    pub(super) state: Mutex<ManagerState>,
    pub(super) terminald: TerminalClient,
    pub(super) daemon_instance_id: Mutex<Option<String>>,
    pub(super) operation: tokio::sync::Mutex<()>,
    pub(super) runtime_sync: tokio::sync::Mutex<()>,
    pub(super) restored_initializations: Mutex<HashSet<String>>,
    pub(super) agent_operations: Mutex<HashMap<String, std::sync::Weak<tokio::sync::RwLock<()>>>>,
    pub(super) hosting_gates: Mutex<HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
    pub(super) hosting_workers: Mutex<HashSet<String>>,
    #[cfg(test)]
    pub(super) clipboard: clipboard::ClipboardStorage,
    #[cfg(test)]
    pub(super) clipboard_operation: tokio::sync::Mutex<()>,
    pub(super) task_stops: tokio::sync::broadcast::Sender<notifications::TaskStopNotification>,
    pub(super) task_completions:
        tokio::sync::broadcast::Sender<notifications::TaskStopNotification>,
    pub(super) notifications_started: std::sync::atomic::AtomicBool,
    pub(super) sync: sync::TerminalSync,
}

#[derive(Default)]
pub(super) struct ManagerState {
    pub(super) tabs: Vec<TerminalTab>,
    pub(super) removing_workspaces: HashSet<String>,
}

impl ManagerState {
    pub(super) fn ensure_workspace_available(&self, path: &str) -> Result<(), TerminalError> {
        if self
            .removing_workspaces
            .iter()
            .any(|root| Path::new(path).starts_with(root))
        {
            return Err(TerminalError::Invalid(
                aow_i18n::text("server.worktree_is_being_cleaned_up_cannot_create_resources_yet")
                    .into(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct PersistedState {
    pub(super) version: u32,
    pub(super) tabs: Vec<TerminalTab>,
}
