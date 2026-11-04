use super::*;
use crate::terminal::notifications::TaskStopNotification;

impl AutomationManager {
    pub(crate) fn hosting_task(&self, id: &str, revision: u64) -> Result<Task> {
        let task = self.task(id)?;
        ensure!(
            task.input.kind == TaskKind::Manual,
            aow_i18n::message("automations.autopilot_supports_only_manual_tasks", &[])
        );
        ensure!(
            task.input.workspace.workspace_mode == WorkspaceMode::Dynamic,
            aow_i18n::message(
                "server.autopilot_supports_only_manual_tasks_with_dynamic_workspaces_change",
                &[]
            )
        );
        ensure!(
            task.revision == revision,
            aow_i18n::message(
                "server.manual_task_changed_take_control_and_select_the_task",
                &[]
            )
        );
        ensure!(
            task.input.prompt_bindings.iter().all(|binding| matches!(
                binding.name.as_str(),
                "workspace" | "session_id" | "turn_id" | "conclusion"
            )),
            aow_i18n::message(
                "server.autopilot_variables_support_only_workspace_session_id_turn_id",
                &[]
            )
        );
        Ok(task)
    }

    pub(crate) async fn dispatch_hosted(
        &self,
        mut task: Task,
        hosting: &aow_protocol::TerminalHosting,
        event: Option<&TaskStopNotification>,
    ) -> Result<()> {
        let run_id = hosting
            .run_id
            .as_deref()
            .context(aow_i18n::text("server.autopilot_run_id_missing"))?;
        ensure!(
            task.input.workspace.workspace_mode == WorkspaceMode::Dynamic,
            aow_i18n::message(
                "server.autopilot_supports_only_manual_tasks_with_dynamic_workspaces",
                &[]
            )
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
                        aow_i18n::text("server.none").into()
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
