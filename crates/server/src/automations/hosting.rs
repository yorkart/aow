use super::*;
use crate::terminal::notifications::TaskStopNotification;

impl AutomationManager {
    pub(crate) fn hosting_task(&self, id: &str, revision: u64) -> Result<Task> {
        let task = self.task(id)?;
        ensure!(task.input.kind == TaskKind::Manual, "托管仅支持手动任务");
        ensure!(
            task.input.workspace.workspace_mode == WorkspaceMode::Dynamic,
            "托管仅支持动态指定工作区的手动任务，请修改任务的工作区方式"
        );
        ensure!(
            task.revision == revision,
            "手动任务已修改，请接管后重新选择任务"
        );
        ensure!(
            task.input.prompt_bindings.iter().all(|binding| matches!(
                binding.name.as_str(),
                "workspace" | "session_id" | "turn_id" | "conclusion"
            )),
            "托管变量仅支持 workspace、session_id、turn_id、conclusion"
        );
        Ok(task)
    }

    pub(crate) async fn dispatch_hosted(
        &self,
        mut task: Task,
        hosting: &aow_protocol::TerminalHosting,
        event: Option<&TaskStopNotification>,
    ) -> Result<()> {
        let run_id = hosting.run_id.as_deref().context("托管运行 ID 缺失")?;
        ensure!(
            task.input.workspace.workspace_mode == WorkspaceMode::Dynamic,
            "托管仅支持动态指定工作区的手动任务"
        );
        task.input
            .resolve_workspace(Some(PathBuf::from(&hosting.process.cwd)))?;
        let variables = task
            .input
            .prompt_bindings
            .iter()
            .map(|binding| {
                let value: &str = match binding.name.as_str() {
                    "workspace" => &hosting.process.cwd,
                    "session_id" => &hosting.session_id,
                    "turn_id" => event
                        .and_then(|event| event.turn_id.as_deref())
                        .unwrap_or_default(),
                    "conclusion" => event
                        .and_then(|event| event.conclusion.as_deref())
                        .unwrap_or_default(),
                    _ => unreachable!("validated hosting variables"),
                };
                (
                    binding.name.clone(),
                    if value.trim().is_empty() {
                        "（无）".into()
                    } else {
                        value.to_owned()
                    },
                )
            })
            .collect();
        self.scheduler
            .dispatch_hosted(&self.store, &task, run_id, variables)
            .await
    }

    pub(crate) fn hosted_run(&self, task_id: &str, run_id: &str) -> Result<Option<Run>> {
        if !self.store.run_path(task_id, run_id)?.exists() {
            return Ok(None);
        }
        self.store.read_run(task_id, run_id)
    }
}
