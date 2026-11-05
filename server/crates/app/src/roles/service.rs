use super::*;
use crate::agents::AgentService;
use crate::host_connection::HostConnection;
use crate::projects::ProjectService;
use crate::settings::persistence::load_registry;
#[derive(Clone)]
pub(crate) struct RoleService {
    pub(super) inner: Arc<RoleState>,
    pub(super) node: HostConnection,
    pub(super) agents: AgentService,
    pub(super) projects: ProjectService,
}
pub(super) struct RoleState {
    pub(super) roles: RoleStore,
    pub(super) config: Option<aow_config::ConfigStore>,
    pub(super) state_dir: Option<PathBuf>,
}
impl RoleService {
    pub(crate) fn new(
        state_dir: Option<PathBuf>,
        config: Option<aow_config::ConfigStore>,
        node: HostConnection,
        agents: AgentService,
        projects: ProjectService,
    ) -> Result<Self, ServiceError> {
        let items = config
            .as_ref()
            .map(|c| load_registry(&c.directory().join(store::FILE)))
            .transpose()?
            .unwrap_or_default();
        let sessions: Vec<AcpRoleSession> = state_dir
            .as_ref()
            .map(|p| load_registry(&p.join(SESSIONS_FILE)))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            node,
            agents,
            projects,
            inner: Arc::new(RoleState {
                config,
                state_dir,
                roles: RoleStore {
                    items: Mutex::new(items),
                    sessions: Mutex::new(
                        sessions
                            .into_iter()
                            .map(|s| (s.thread_id.clone(), s))
                            .collect(),
                    ),
                    ..Default::default()
                },
            }),
        })
    }
}
