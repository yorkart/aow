use aow_agents::automation::{AgentAutomation, SessionIdMode};
use aow_agents::environment;
use std::{
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use chrono::Utc;
use tempfile::TempDir;
use tokio::{io::AsyncWriteExt, process::Command, sync::mpsc};
use uuid::Uuid;

use crate::{
    Run, RunEvent, RunOutput, RunSource, RunStatus, Store, Task, WorkspaceMode, agent,
    store::{RunWriter, new_run_id, private_dir},
    task_lock::ConcurrencySlot,
};

/// Keeps cancellation from leaving an unobserved agent or shell running.
struct ProcessGroup(u32);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

fn millis(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

pub fn initial_run(task: &Task, id: String, source: RunSource) -> Run {
    Run {
        id,
        task_id: task.id.clone(),
        task_revision: task.revision,
        task_name: task.input.name.clone(),
        agent: task.input.agent,
        source,
        variables: (source == RunSource::Manual).then(Default::default),
        status: RunStatus::Preparing,
        started_at: Utc::now(),
        finished_at: None,
        workspace_path: None,
        branch: None,
        session_id: None,
        agent_pid: None,
        agent_command: None,
        exit_code: None,
        message: None,
        preparation_ms: None,
        session_acquired_ms: None,
        duration_ms: None,
    }
}

fn command_argv(command: &Command) -> Vec<String> {
    let command = command.as_std();
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|part| part.to_string_lossy().into_owned())
        .collect()
}

struct PreparedWorkspace {
    directory: PathBuf,
    branch: Option<String>,
    // Keeping TempDir alive makes the directory available to the Agent for the
    // full run, then removes it on every normal return or cancellation path.
    _temporary_directory: Option<TempDir>,
}

pub async fn run(
    store: &Store,
    task_id: &str,
    id: Option<String>,
    source: RunSource,
) -> Result<RunStatus> {
    let mut task = store.get_task(task_id)?;
    let deleted = task.deleted;
    let mut variables = Default::default();
    if source == RunSource::Manual {
        if let Some(request) = id
            .as_deref()
            .map(|id| store.take_manual_request(task_id, id))
            .transpose()?
            .flatten()
        {
            task = request.task;
            variables = request.variables;
        }
    }
    task.launch.environment.remove("PATH");
    let mut run = initial_run(&task, id.unwrap_or_else(new_run_id), source);
    if source == RunSource::Manual {
        run.variables = Some(variables.clone());
    }
    let started = Instant::now();
    let Some(mut concurrency_slot) =
        store.concurrency_slot(&task.id, task.input.max_concurrent_runs)?
    else {
        return Ok(RunStatus::Skipped);
    };
    concurrency_slot.begin(&run.id)?;
    let mut writer = store.create_run(&run, &task)?;
    let mut outcome = if deleted
        || task.deleted
        || (source == RunSource::Scheduled
            && (!task.input.enabled || task.input.kind == crate::TaskKind::Manual))
    {
        Ok((RunStatus::Skipped, None, Some("任务已暂停或删除".into())))
    } else {
        tokio::select! {
            result = async {
                // Keep the original template and bindings in history. Resolve
                // only this in-memory execution copy, without parsing variables.
                task.input.prompt = task.input.render_prompt(&variables)?;
                task.input.prompt_bindings.clear();
                let mut agent_environment = environment::load_agent_environment_from(
                    &store.config_dir,
                    task.input.agent.id(),
                ).await?;
                // PATH is always loaded from the shared execution settings, even
                // when an Agent registration contains its own PATH override.
                agent_environment.remove("PATH");
                let paths = environment::load_path_from(&store.config_dir).await?;
                task.launch.environment.insert("PATH".into(), environment::path_value(&paths)?);
                execute(
                    store,
                    &task,
                    &agent_environment,
                    &run.id,
                    &mut writer,
                    started,
                    &concurrency_slot,
                ).await
            } => result,
            _ = shutdown_signal() => Ok((RunStatus::Interrupted, None, Some("执行收到停止信号".into()))),
        }
    };
    if let Err(error) = cleanup_worktree(store, &task, &run.id, &concurrency_slot).await {
        outcome = cleanup_failure(outcome, error);
    }
    let (status, exit_code, message) = match outcome {
        Ok(outcome) => outcome,
        Err(error) => (
            RunStatus::Failed,
            None,
            Some(format!("{error:#}").chars().take(2000).collect()),
        ),
    };
    writer.append(&RunEvent::Finished {
        at: Utc::now(),
        status,
        exit_code,
        message,
        duration_ms: millis(started),
    })?;
    Ok(status)
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    if let Ok(mut terminate) = signal(SignalKind::terminate()) {
        tokio::select! { _ = terminate.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
    } else {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn git(task: &Task, cwd: &Path, args: &[&str], slot: &ConcurrencySlot) -> Result<String> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .envs(&task.launch.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    slot.register(&mut command);
    let child = command.spawn()?;
    let _group = ProcessGroup(child.id().unwrap());
    let output = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output())
        .await
        .context("Git 操作超时")??;
    ensure!(
        output.status.success(),
        "Git 操作失败: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(1000)
            .collect::<String>()
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

async fn workspace(
    store: &Store,
    task: &Task,
    run_id: &str,
    concurrency_slot: &ConcurrencySlot,
) -> Result<PreparedWorkspace> {
    if task.input.workspace_mode == WorkspaceMode::Temporary {
        let temporary = tempfile::Builder::new()
            .prefix(&format!("aow-automation-{}-{run_id}-", task.id))
            .tempdir()
            .context("无法创建临时工作区")?;
        return Ok(PreparedWorkspace {
            directory: temporary.path().to_path_buf(),
            branch: None,
            _temporary_directory: Some(temporary),
        });
    }
    let root = task
        .repository_path
        .canonicalize()
        .context("项目目录不存在")?;
    let requested = task
        .input
        .workspace_path
        .canonicalize()
        .context("工作区目录不存在")?;
    let root_common = git(
        task,
        &root,
        &["rev-parse", "--git-common-dir"],
        concurrency_slot,
    )
    .await?;
    let worktree_common = git(
        task,
        &requested,
        &["rev-parse", "--git-common-dir"],
        concurrency_slot,
    )
    .await?;
    ensure!(
        root.join(root_common).canonicalize()? == requested.join(worktree_common).canonicalize()?,
        "工作区不属于该项目"
    );
    let directory = match task.input.workspace_mode {
        WorkspaceMode::NewWorktree => {
            let directory = store.root.join("worktrees").join(&task.id).join(run_id);
            private_dir(directory.parent().unwrap())?;
            let branch = format!("automation/{}/{}", task.id, run_id);
            let base = git(
                task,
                &root,
                &[
                    "rev-parse",
                    "--verify",
                    &format!("{}^{{commit}}", task.input.base_branch),
                ],
                concurrency_slot,
            )
            .await?;
            git(
                task,
                &root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &branch,
                    directory.to_str().context("工作区路径不是 UTF-8")?,
                    &base,
                ],
                concurrency_slot,
            )
            .await?;
            directory
        }
        WorkspaceMode::Existing | WorkspaceMode::NewBranch => {
            if task.input.workspace_mode == WorkspaceMode::NewBranch {
                ensure!(
                    git(
                        task,
                        &requested,
                        &["status", "--porcelain"],
                        concurrency_slot
                    )
                    .await?
                    .is_empty(),
                    "工作区有未提交修改，无法创建并切换分支"
                );
                let branch = format!("automation/{}/{}", task.id, run_id);
                let base = git(
                    task,
                    &root,
                    &[
                        "rev-parse",
                        "--verify",
                        &format!("{}^{{commit}}", task.input.base_branch),
                    ],
                    concurrency_slot,
                )
                .await?;
                git(
                    task,
                    &requested,
                    &["checkout", "-b", &branch, &base],
                    concurrency_slot,
                )
                .await?;
            }
            requested
        }
        WorkspaceMode::Temporary => unreachable!("temporary workspaces return before Git setup"),
    };
    let branch = git(
        task,
        &directory,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        concurrency_slot,
    )
    .await?;
    Ok(PreparedWorkspace {
        directory,
        branch: Some(branch),
        _temporary_directory: None,
    })
}

type Outcome = (RunStatus, Option<i32>, Option<String>);

fn cleanup_failure(outcome: Result<Outcome>, error: anyhow::Error) -> Result<Outcome> {
    let cleanup = format!("清理 Worktree 失败: {error:#}");
    match outcome {
        Ok((_, exit_code, message)) => Ok((
            RunStatus::Failed,
            exit_code,
            Some(match message {
                Some(message) => format!("{message}; {cleanup}"),
                None => cleanup,
            }),
        )),
        Err(error) => Err(anyhow::anyhow!("{error:#}; {cleanup}")),
    }
}

async fn cleanup_worktree(
    store: &Store,
    task: &Task,
    run_id: &str,
    concurrency_slot: &ConcurrencySlot,
) -> Result<()> {
    if task.input.workspace_mode != WorkspaceMode::NewWorktree {
        return Ok(());
    }
    let directory = store.root.join("worktrees").join(&task.id).join(run_id);
    if !directory.exists() {
        return Ok(());
    }
    let root = task
        .repository_path
        .canonicalize()
        .context("项目目录不存在，无法清理 Worktree")?;
    git(
        task,
        &root,
        &[
            "worktree",
            "remove",
            "--force",
            directory.to_str().context("工作区路径不是 UTF-8")?,
        ],
        concurrency_slot,
    )
    .await?;
    Ok(())
}

async fn execute(
    store: &Store,
    task: &Task,
    agent_environment: &std::collections::BTreeMap<String, String>,
    id: &str,
    writer: &mut RunWriter,
    started: Instant,
    concurrency_slot: &ConcurrencySlot,
) -> Result<Outcome> {
    task.input.validate()?;
    let workspace = workspace(store, task, id, concurrency_slot).await?;
    let directory = &workspace.directory;
    writer.append(&RunEvent::Workspace {
        path: directory.clone(),
        branch: workspace.branch.clone(),
        elapsed_ms: millis(started),
    })?;
    if !task.input.precheck_command.trim().is_empty() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &task.input.precheck_command])
            .current_dir(directory)
            .envs(&task.launch.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        concurrency_slot.register(&mut command);
        let mut child = command.spawn()?;
        let _group = ProcessGroup(child.id().unwrap());
        let status = tokio::time::timeout(
            Duration::from_secs(task.input.precheck_timeout_seconds),
            child.wait(),
        )
        .await
        .context("执行前检查超时")??;
        if !status.success() {
            return Ok((
                RunStatus::Skipped,
                status.code(),
                Some("执行前检查未通过".into()),
            ));
        }
    }
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
    let mut child = command.spawn().context("无法启动 Agent")?;
    let _group = ProcessGroup(child.id().unwrap());
    writer.append(&RunEvent::AgentStarted {
        pid: child.id().unwrap(),
        command: agent_command,
    })?;
    let stdin = child.stdin.take();
    let prompt = task.input.prompt.clone();
    let input = tokio::spawn(async move {
        if let Some(mut stdin) = stdin {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.shutdown().await?;
        }
        Ok::<_, std::io::Error>(())
    });
    let stdout = child.stdout.take().context("Agent 未提供标准输出")?;
    let stderr = child.stderr.take().context("Agent 未提供错误输出")?;
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
    let output = tokio::spawn(agent::capture_stdout(stdout, stdout_file, stdout_reporter));
    let errors = tokio::spawn(agent::capture_stderr(stderr, stderr_file, reporter));
    drop(tx);
    let mut session_received = specified_id.is_some();
    let mut channel_open = true;
    let session_deadline = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(session_deadline);
    let status: Result<ExitStatus> = loop {
        tokio::select! {
            session = rx.recv(), if channel_open => match session {
                Some(session_id) => { writer.append(&RunEvent::Session { session_id, elapsed_ms: millis(started) })?; session_received = true; }
                None => channel_open = false,
            },
            status = child.wait() => break status.map_err(Into::into),
            _ = &mut session_deadline, if !session_received && session_mode == SessionIdMode::FromOutput => break Err(anyhow::anyhow!("Agent 启动 60 秒内未返回会话 ID")),
        }
    };
    // Descendants must not keep inherited pipes open after the agent exits or
    // after the session-ID deadline aborts the run.
    drop(_group);
    let output = output.await??;
    // Hermes reports its identity on stderr at exit. Drain both readers before
    // consuming the channel, otherwise the final session line races validation.
    let stderr = errors.await??;
    while let Ok(session_id) = rx.try_recv() {
        writer.append(&RunEvent::Session {
            session_id,
            elapsed_ms: millis(started),
        })?;
        session_received = true;
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
                    format!("Agent 退出: {status}")
                } else {
                    stderr.stderr_tail.unwrap_or_default()
                },
            ),
        ));
    }
    input_result.context("无法将任务内容发送给 Agent")?;
    ensure!(session_received, "Agent 已退出，但没有返回会话 ID");
    Ok((RunStatus::Completed, status.code(), None))
}
