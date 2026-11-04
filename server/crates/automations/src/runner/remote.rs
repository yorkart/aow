use super::*;
use aow_runtime_client::{ExecutionReference, RuntimeClient, RuntimeError};
use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Write, time::Duration};
const REFERENCE: &str = "node-execution.json";
fn client(store: &Store) -> RuntimeClient {
    RuntimeClient::from_connection_file(store.state_dir.join("runtime/runtime-client.json"))
}
pub fn execution_reference(
    store: &Store,
    task_id: &str,
    id: &str,
) -> Result<Option<ExecutionReference>> {
    let path = store.run_path(task_id, id)?.join(REFERENCE);
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn save_reference(store: &Store, task_id: &str, reference: &ExecutionReference) -> Result<()> {
    crate::store::atomic_write(
        &store.run_path(task_id, &reference.id)?.join(REFERENCE),
        &serde_json::to_vec(reference)?,
    )
}
fn consumed(store: &Store, task_id: &str, id: &str) -> Result<u64> {
    let bytes = std::fs::read(store.run_path(task_id, id)?.join("events.jsonl"))?;
    Ok(bytes
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| line.ends_with(b"\n"))
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .filter_map(|event| event["_node_sequence"].as_u64())
        .max()
        .unwrap_or(0))
}
async fn copy_output(
    client: &RuntimeClient,
    store: &Store,
    task_id: &str,
    reference: &ExecutionReference,
    output: crate::RunOutput,
) -> Result<()> {
    let path = store.run_output_path(task_id, &reference.id, output)?;
    let mut file = OpenOptions::new().append(true).open(&path)?;
    let mut offset = file.metadata()?.len();
    loop {
        let bytes = client
            .execution_output(reference, output == crate::RunOutput::Stderr, offset)
            .await?;
        if bytes.is_empty() {
            break;
        }
        file.write_all(&bytes)?;
        file.sync_data()?;
        offset += bytes.len() as u64;
    }
    Ok(())
}
async fn sync(
    client: &RuntimeClient,
    store: &Store,
    task_id: &str,
    reference: &mut ExecutionReference,
    writer: &mut crate::store::RunWriter,
) -> Result<Option<RunStatus>> {
    let after = consumed(store, task_id, &reference.id)?;
    let snapshot = match client.execution(reference, after).await {
        Ok(snapshot) => snapshot,
        Err(RuntimeError::HttpStatus { status, .. }) if status == http::StatusCode::NOT_FOUND => {
            let spec: Value = serde_json::from_slice(&std::fs::read(
                store
                    .run_path(task_id, &reference.id)?
                    .join("node-spec.json"),
            )?)?;
            client.submit_execution(reference, spec).await?;
            client.execution(reference, after).await?
        }
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        snapshot.node_id == reference.node_id && snapshot.id == reference.id,
        "Execution belongs to a different node"
    );
    for output in crate::RunOutput::ALL {
        copy_output(client, store, task_id, reference, output).await?;
    }
    if let Some(session) = snapshot.native_session {
        reference.native_session = Some(session.reference);
        save_reference(store, task_id, reference)?;
    }
    if let Some(result) = snapshot.result {
        crate::store::atomic_write(
            &store.run_path(task_id, &reference.id)?.join("result.json"),
            &serde_json::to_vec(&result)?,
        )?;
    }
    let mut status = None;
    for (expected, item) in (after + 1..).zip(snapshot.events) {
        anyhow::ensure!(
            item.sequence == expected,
            "Execution event sequence has a gap"
        );
        let event: RunEvent = serde_json::from_value(item.event.clone())?;
        if let RunEvent::Finished {
            status: finished, ..
        } = event
        {
            status = Some(finished);
        }
        let mut value = item.event;
        value["_node_sequence"] = json!(item.sequence);
        writer.append(&value)?;
    }
    anyhow::ensure!(
        !snapshot.terminal || status.is_some(),
        "Terminal execution has no completion event"
    );
    if status.is_some() && client.remove_execution(reference).await.is_ok() {
        crate::store::atomic_write(
            &store.run_path(task_id, &reference.id)?.join("node-cleaned"),
            b"",
        )?;
    }
    Ok(status)
}
async fn await_completion(
    client: &RuntimeClient,
    store: &Store,
    task_id: &str,
    reference: &mut ExecutionReference,
    writer: &mut crate::store::RunWriter,
) -> Result<RunStatus> {
    loop {
        match sync(client, store, task_id, reference, writer).await {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) if error.downcast_ref::<RuntimeError>().is_some_and(transient) => {
                tracing::warn!(%error,"execution node is unavailable; keeping the run for reconnect")
            }
            Err(error) => return Err(error),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
fn transient(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Http(_) | RuntimeError::Io(_) | RuntimeError::WebSocket(_)
    ) || matches!(error,RuntimeError::HttpStatus{status,..} if status.is_server_error())
}
async fn submit_intent(
    client: &RuntimeClient,
    reference: &ExecutionReference,
    spec: Value,
) -> Result<()> {
    loop {
        match client.submit_execution(reference, spec.clone()).await {
            Ok(()) => return Ok(()),
            Err(RuntimeError::HttpStatus { status, .. }) if status == http::StatusCode::GONE => {
                return Ok(());
            }
            Err(error) if transient(&error) => {
                tracing::warn!(%error,"execution submission uncertain; retrying the same execution ID")
            }
            Err(error) => return Err(error.into()),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
async fn cancel_intent(
    client: &RuntimeClient,
    store: &Store,
    task_id: &str,
    reference: &ExecutionReference,
) -> Result<()> {
    crate::store::atomic_write(
        &store
            .run_path(task_id, &reference.id)?
            .join("node-cancel-requested"),
        b"",
    )?;
    loop {
        match client.cancel_execution(reference).await {
            Ok(()) => return Ok(()),
            Err(error) if transient(&error) => {
                tracing::warn!(%error,"execution cancellation pending until node reconnects")
            }
            Err(error) => return Err(error.into()),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
pub(super) async fn run(
    store: &Store,
    task_id: &str,
    id: Option<String>,
    source: RunSource,
) -> Result<RunStatus> {
    let mut task = store.get_task(task_id)?;
    let mut variables = Default::default();
    let mut hosted = false;
    if source == RunSource::Manual
        && let Some(request) = id
            .as_deref()
            .map(|id| store.take_manual_request(task_id, id))
            .transpose()?
            .flatten()
    {
        task = request.task;
        variables = request.variables;
        hosted = request.hosted;
    }
    let mut run = super::initial_run(&task, id.unwrap_or_else(new_run_id), source);
    if source == RunSource::Manual {
        run.variables = Some(variables.clone());
    }
    let started = Instant::now();
    let Some(mut slot) = store.concurrency_slot(&task.id, task.input.max_concurrent_runs)? else {
        if hosted {
            let mut writer = store.create_run(&run, &task)?;
            writer.append(&RunEvent::Finished {
                at: Utc::now(),
                status: RunStatus::Skipped,
                exit_code: None,
                message: Some("Task concurrency limit is occupied".into()),
                duration_ms: 0,
            })?;
        }
        return Ok(RunStatus::Skipped);
    };
    slot.begin(&run.id)?;
    let mut writer = store.create_run(&run, &task)?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let client = client(store);
    let outcome = tokio::select! {
        outcome=async {
            if task.deleted || (source==RunSource::Scheduled && (!task.input.enabled || task.input.kind==crate::TaskKind::Manual)) {return Ok(RunStatus::Skipped);}
            let mut prompt=task.input.render_prompt(&variables)?;
            if hosted {prompt=crate::hosting::prompt(&prompt);}
            let health=client.health().await?;
            let node_id=health["node_id"].as_str().context("Runtime node ID is unavailable")?.to_owned();
            let mut environment=aow_agents::environment::load_agent_environment_from(&store.config_dir,task.input.agent_id()).await?;
            environment.remove("PATH");
            let callback=aow_cli_access::Access::from_state(&store.state_dir)?.environment()?;
            environment.extend(callback);
            let mut reference=ExecutionReference{node_id,id:run.id.clone(),native_session:None};
            let spec=json!({
                "id":run.id,"repository_path":task.repository_path,"launch":task.launch,
                "input":{"agent":task.input.agent,"prompt":prompt,"yolo":task.input.yolo,
                    "workspace_mode":task.input.workspace.workspace_mode,"workspace_path":task.input.workspace.workspace_path,"base_branch":task.input.workspace.base_branch},
                "agent_environment":environment,"capacity":{"key":task.id,"limit":task.input.max_concurrent_runs}
            });
            crate::store::atomic_write(&store.run_path(task_id,&run.id)?.join("node-spec.json"),&serde_json::to_vec(&spec)?)?;
            save_reference(store,task_id,&reference)?;
            if let Err(error)=submit_intent(&client,&reference,spec).await {
                if matches!(error.downcast_ref::<RuntimeError>(),Some(RuntimeError::HttpStatus{status,error:Some(error),..}) if *status==http::StatusCode::CONFLICT&&error.code=="execution_busy") {return Ok(RunStatus::Skipped);}
                return Err(error);
            }
            await_completion(&client,store,task_id,&mut reference,&mut writer).await
        }=>Some(outcome),
        _=terminate.recv()=>None,
        _=tokio::signal::ctrl_c()=>None,
    };
    let result = match outcome {
        Some(result) => result,
        None => match execution_reference(store, task_id, &run.id)? {
            Some(mut reference) => {
                cancel_intent(&client, store, task_id, &reference).await?;
                await_completion(&client, store, task_id, &mut reference, &mut writer).await
            }
            None => Ok(RunStatus::Interrupted),
        },
    };
    match result {
        Ok(status)
            if store
                .read_run(task_id, &run.id)?
                .is_some_and(|run| run.status.terminal()) =>
        {
            Ok(status)
        }
        result => {
            let (status, message) = match result {
                Ok(status) => (status, None),
                Err(error) => (RunStatus::Failed, Some(format!("{error:#}"))),
            };
            writer.append(&RunEvent::Finished {
                at: Utc::now(),
                status,
                exit_code: None,
                message,
                duration_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            })?;
            Ok(status)
        }
    }
}
/// Reattach orphaned caller journals to their still-owned node execution.
pub async fn reconcile(store: &Store) -> Result<()> {
    let client = client(store);
    for task in store.all_tasks()? {
        for id in store.run_ids(&task.id, None)? {
            let result: Result<()> = async {
                let Some(mut reference) = execution_reference(store, &task.id, &id)? else {
                    return Ok(());
                };
                let Some(run) = store.read_run(&task.id, &id)? else {
                    return Ok(());
                };
                if run.status.terminal() {
                    if !store
                        .run_path(&task.id, &id)?
                        .join("node-cleaned")
                        .is_file()
                        && client.remove_execution(&reference).await.is_ok()
                    {
                        crate::store::atomic_write(
                            &store.run_path(&task.id, &id)?.join("node-cleaned"),
                            b"",
                        )?;
                    }
                    return Ok(());
                }
                let Some(mut writer) = store.resume_run_writer(&task.id, &id)? else {
                    return Ok(());
                };
                if store
                    .run_path(&task.id, &id)?
                    .join("node-cancel-requested")
                    .is_file()
                {
                    client.cancel_execution(&reference).await?;
                }
                let _ = sync(&client, store, &task.id, &mut reference, &mut writer).await?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(task_id=%task.id,run_id=%id,%error,"cannot reconcile node execution");
            }
        }
    }
    Ok(())
}
