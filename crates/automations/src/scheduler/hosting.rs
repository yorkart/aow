use super::*;

impl Scheduler {
    /// The caller persists this ID before dispatch so every outcome can be
    /// associated with its hosting round, including native scheduler failures.
    pub async fn dispatch_hosted(
        &self,
        store: &Store,
        task: &Task,
        id: &str,
        variables: BTreeMap<String, String>,
    ) -> Result<()> {
        let result = async {
            ensure!(task.input.kind == TaskKind::Manual, "托管仅支持手动任务");
            task.input.render_prompt(&variables)?;
            ensure!(
                store
                    .concurrency_slot(&task.id, task.input.max_concurrent_runs)?
                    .is_some(),
                "手动任务正在执行，已达到并发限制"
            );
            store.save_manual_request(
                id,
                &ManualRunRequest {
                    task: task.clone(),
                    variables: variables.clone(),
                    hosted: true,
                },
            )?;
            self.dispatch_run(store, task, RunSource::Manual, id).await
        }
        .await;
        if let Err(error) = &result {
            let _ = store.take_manual_request(&task.id, id);
            let mut run = crate::runner::initial_run(task, id.into(), RunSource::Manual);
            run.variables = Some(variables);
            if let Ok(mut writer) = store.create_run(&run, task) {
                writer.append(&crate::RunEvent::Finished {
                    at: chrono::Utc::now(),
                    status: crate::RunStatus::Failed,
                    exit_code: None,
                    message: Some(format!("托管任务启动失败: {error:#}")),
                    duration_ms: 0,
                })?;
            }
        }
        result
    }
}
