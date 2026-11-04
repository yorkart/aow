mod run;
mod task;

pub use aow_runtime_contract::agents::AgentType as AgentKind;
pub use aow_workspaces::{WorkspaceConfig, WorkspaceMode};
pub use run::{Run, RunEvent, RunOutput, RunSource, RunStatus};
pub use task::{
    AgentLaunch, FailureNotification, ManualRunRequest, PromptBinding, Task, TaskInput, TaskKind,
};
