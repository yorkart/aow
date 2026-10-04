//! Runtime registry and daemon shutdown coordination.

use super::*;

mod lifecycle;
mod registry;
mod spawn;

pub(super) use spawn::{SpawnPermit, SpawnTracker};

#[derive(Clone)]
pub(super) struct DaemonState {
    pub(super) inner: Arc<DaemonInner>,
}

pub(super) struct DaemonInner {
    pub(super) instance_id: String,
    pub(super) shutting_down: AtomicBool,
    pub(super) in_flight_spawns: Arc<SpawnTracker>,
    pub(super) runtimes: Mutex<HashMap<String, Arc<Runtime>>>,
    pub(super) vt_worker: Option<VtWorkerClient>,
    pub(super) agent_cache: Mutex<agents::AgentCache>,
}

pub(super) enum CreateOutcome {
    Created(TerminalRuntime),
    Existing(TerminalRuntime),
}
