//! Single-execution ownership, cancellation and output capture. Scheduling belongs to Server.
mod process;
pub use process::{ProcessResult, ProcessSpec, run_once};

mod manager;
mod model;
pub use manager::ExecutionManager;
mod runner;
mod store;
mod tracking;
use model::Task;
use store::Store;
use tracking as task_lock;
mod agent;
pub use model::{
    AgentKind, AgentLaunch, CapacityReservation, ExecutionInput, ExecutionSpec, RunEvent,
    RunOutput, RunStatus, WorkspaceMode,
};
