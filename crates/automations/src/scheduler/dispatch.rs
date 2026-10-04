use super::*;

impl Scheduler {
    pub async fn dispatch(
        &self,
        store: &Store,
        task: &Task,
        source: RunSource,
    ) -> Result<Option<String>> {
        self.dispatch_with_variables(store, task, source, BTreeMap::new())
            .await
    }

    pub async fn dispatch_with_variables(
        &self,
        store: &Store,
        task: &Task,
        source: RunSource,
        variables: BTreeMap<String, String>,
    ) -> Result<Option<String>> {
        ensure!(
            source == RunSource::Manual || task.input.kind == TaskKind::Scheduled,
            "手动任务不能定时触发"
        );
        task.input.render_prompt(&variables)?;
        ensure!(
            task.input.workspace.workspace_mode != crate::WorkspaceMode::Dynamic,
            "请在执行时指定工作区目录"
        );
        // This avoids creating a transient OS job when all slots are occupied.
        // The runner acquires a slot again as the authoritative race-free check.
        if store
            .concurrency_slot(&task.id, task.input.max_concurrent_runs)?
            .is_none()
        {
            return Ok(None);
        }
        let id = new_run_id();
        if source == RunSource::Manual {
            store.save_manual_request(
                &id,
                &ManualRunRequest {
                    task: task.clone(),
                    variables: variables.clone(),
                    hosted: false,
                },
            )?;
        }
        if let Err(error) = self.dispatch_run(store, task, source, &id).await {
            if source == RunSource::Manual {
                let _ = store.take_manual_request(&task.id, &id);
            }
            // If the native manager failed before a runner claimed this ID,
            // leave a failed execution record. create_run never overwrites.
            let mut run = crate::runner::initial_run(task, id.clone(), source);
            if source == RunSource::Manual {
                run.variables = Some(variables);
            }
            if let Ok(mut writer) = store.create_run(&run, task) {
                writer.append(&crate::RunEvent::Finished {
                    at: chrono::Utc::now(),
                    status: crate::RunStatus::Failed,
                    exit_code: None,
                    message: Some(format!("Runner 启动失败: {error:#}")),
                    duration_ms: 0,
                })?;
            }
            return Err(error);
        }
        Ok(Some(id))
    }

    pub(crate) async fn dispatch_run(
        &self,
        store: &Store,
        task: &Task,
        source: RunSource,
        id: &str,
    ) -> Result<()> {
        let domain = self.check_ready().await?;
        ensure!(
            !task.deleted && (source == RunSource::Manual || task.input.enabled),
            "任务已暂停或删除"
        );
        let mut args = self.arguments(store, task, "run", source);
        args.extend(["--run-id".into(), id.to_owned()]);
        let label = format!("{}.run.{id}", self.label(&task.id));
        if self.platform == Platform::Systemd {
            let mut launch = vec![
                "--user".into(),
                "--quiet".into(),
                "--collect".into(),
                "--service-type=exec".into(),
                format!("--unit={label}"),
                "--property=UMask=0077".into(),
                "--property=StandardOutput=null".into(),
                "--property=StandardError=journal".into(),
                "--".into(),
            ];
            launch.extend(args.into_iter().map(|arg| arg.replace('$', "$$")));
            self.command(&self.dispatch_command, &launch).await?;
        } else {
            self.dispatch_launchd(
                store,
                task,
                &label,
                &args,
                domain.as_deref().context("缺少 launchd 用户域")?,
            )
            .await?;
        }
        Ok(())
    }
}
