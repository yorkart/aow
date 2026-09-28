mod run;
mod task;

pub use aow_agents::automation::AutomationAgent as AgentKind;
pub use run::{Run, RunEvent, RunOutput, RunSource, RunStatus};
pub use task::{
    AgentLaunch, FailureNotification, ManualRunRequest, PromptBinding, Task, TaskInput, TaskKind,
    WorkspaceMode,
};
