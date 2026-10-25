use aow_agents::automation::{AgentAutomation, SessionIdMode};
use std::{
    process::ExitStatus,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tokio::{io::AsyncWriteExt, process::Command, sync::mpsc};
use uuid::Uuid;

use crate::{
    RunEvent, RunOutput, RunStatus, Store, Task, agent, store::RunWriter,
    task_lock::ConcurrencySlot,
};

use super::{Outcome, millis, process_group::ProcessGroup, workspace};

fn command_argv(command: &Command) -> Vec<String> {
    let command = command.as_std();
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|part| part.to_string_lossy().into_owned())
        .collect()
}
pub(super) async fn execute(
    store: &Store,
    task: &Task,
    agent_environment: &std::collections::BTreeMap<String, String>,
    id: &str,
    writer: &mut RunWriter,
    started: Instant,
    concurrency_slot: &ConcurrencySlot,
) -> Result<Outcome> {
    task.input.validate()?;
    let workspace = workspace::prepare(task, id, concurrency_slot).await?;
    let directory = &workspace.directory;
    writer.append(&RunEvent::Workspace {
        path: directory.clone(),
        branch: workspace.branch.clone(),
        elapsed_ms: millis(started),
    })?;
    let session_mode = task.input.agent.session_id_mode();
    let specified_id =
        (session_mode == SessionIdMode::GeneratedUuid).then(|| Uuid::new_v4().to_string());
    if let Some(session_id) = &specified_id {
        writer.append(&RunEvent::Session {
            session_id: session_id.clone(),
            elapsed_ms: millis(started),
        })?;
    }
    let mut command = agent::command(task, agent_environment, directory, specified_id.as_deref())?;
    concurrency_slot.register(&mut command);
    let agent_command = command_argv(&command);
    store.capture_session_roots(&task.id, id, command.as_std())?;
    let mut child = command
        .spawn()
        .context(aow_i18n::text("automations.unable_to_start_agent"))?;
    let _group = ProcessGroup(child.id().unwrap());
    writer.append(&RunEvent::AgentStarted {
        pid: child.id().unwrap(),
        command: agent_command,
    })?;
    let stdin = child.stdin.take();
    let prompt = task.input.prompt.clone();
    let input = aow_i18n::spawn(async move {
        if let Some(mut stdin) = stdin {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.shutdown().await?;
        }
        Ok::<_, std::io::Error>(())
    });
    let stdout = child
        .stdout
        .take()
        .context(aow_i18n::text("automations.agent_did_not_provide_stdout"))?;
    let stderr = child
        .stderr
        .take()
        .context(aow_i18n::text("automations.agent_did_not_provide_stderr"))?;
    let stdout_file = tokio::fs::File::from_std(writer.output_writer(RunOutput::Stdio)?);
    let stderr_file = tokio::fs::File::from_std(writer.output_writer(RunOutput::Stderr)?);
    let (tx, mut rx) = mpsc::channel(1);
    let session_reported = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reporter = specified_id
        .is_none()
        .then(|| (task.input.agent, tx.clone(), session_reported));
    let stdout_reporter = if session_mode == SessionIdMode::FromStderrOnExit {
        None
    } else {
        reporter.clone()
    };
    let output = aow_i18n::spawn(agent::capture_stdout(stdout, stdout_file, stdout_reporter));
    let errors = aow_i18n::spawn(agent::capture_stderr(stderr, stderr_file, reporter));
    drop(tx);
    let mut received_session = specified_id;
    let mut channel_open = true;
    let session_deadline = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(session_deadline);
    let status: Result<ExitStatus> = loop {
        tokio::select! {
            session = rx.recv(), if channel_open => match session {
                Some(session_id) => { received_session = Some(session_id.clone()); writer.append(&RunEvent::Session { session_id, elapsed_ms: millis(started) })?; }
                None => channel_open = false,
            },
            status = child.wait() => break status.map_err(Into::into),
            _ = &mut session_deadline, if received_session.is_none() && session_mode == SessionIdMode::FromOutput => break Err(anyhow::anyhow!(aow_i18n::message("automations.agent_did_not_return_a_session_id_within_60", &[]))),
        }
    };
    let exited_at = chrono::Utc::now();
    // Descendants must not keep inherited pipes open after the agent exits or
    // after the session-ID deadline aborts the run.
    drop(_group);
    let output = output.await??;
    // Hermes reports its identity on stderr at exit. Drain both readers before
    // consuming the channel, otherwise the final session line races validation.
    let stderr = errors.await??;
    while let Ok(session_id) = rx.try_recv() {
        received_session = Some(session_id.clone());
        writer.append(&RunEvent::Session {
            session_id,
            elapsed_ms: millis(started),
        })?;
    }
    if output.truncated {
        writer.append(&RunEvent::OutputTruncated {
            output: RunOutput::Stdio,
            limit_bytes: crate::store::MAX_RUN_OUTPUT_BYTES,
        })?;
    }
    if stderr.truncated {
        writer.append(&RunEvent::OutputTruncated {
            output: RunOutput::Stderr,
            limit_bytes: crate::store::MAX_RUN_OUTPUT_BYTES,
        })?;
    }
    let status = match status {
        Ok(status) => status,
        // Do not replace the session-ID failure with a best-effort stdin error
        // caused by killing the agent process group.
        Err(error) => {
            let _ = input.await;
            return Err(error);
        }
    };
    let input_result = input.await?;
    if !status.success() {
        return Ok((
            RunStatus::Failed,
            status.code(),
            Some(
                if stderr.stderr_tail.as_deref().unwrap_or_default().is_empty() {
                    aow_i18n::message("automations.agent_exited", &[("p0", format!("{}", status))])
                } else {
                    stderr.stderr_tail.unwrap_or_default()
                },
            ),
        ));
    }
    input_result.context(aow_i18n::text(
        "automations.unable_to_send_task_content_to_agent",
    ))?;
    let session_id = received_session.context(aow_i18n::text(
        "automations.agent_exited_without_returning_a_session_id",
    ))?;
    super::result::capture(store, task, id, session_id, exited_at).await?;
    Ok((RunStatus::Completed, status.code(), None))
}
