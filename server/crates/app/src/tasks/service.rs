use super::*;
use crate::roles::RoleService;
use crate::runtime_connection::RuntimeConnection;
#[derive(Clone)]
pub(crate) struct TaskService {
    pub(super) inner: Arc<TaskState>,
    pub(super) roles: RoleService,
    pub(super) node: RuntimeConnection,
}
pub(super) struct TaskState {
    pub(super) tasks: aow_db::tasks::TaskDb,
    pub(super) task_operation: tokio::sync::Mutex<()>,
    pub(super) state_dir: Option<PathBuf>,
}
impl TaskService {
    pub(crate) fn new(
        state_dir: Option<PathBuf>,
        roles: RoleService,
        node: RuntimeConnection,
    ) -> Result<Self, ServiceError> {
        let path = state_dir.as_ref().map(|p| p.join("state/tasks.sqlite3"));
        let tasks =
            aow_db::tasks::TaskDb::open(path.as_deref()).map_err(ServiceError::Configuration)?;
        Ok(Self {
            roles,
            node,
            inner: Arc::new(TaskState {
                tasks,
                state_dir,
                task_operation: Default::default(),
            }),
        })
    }
}
