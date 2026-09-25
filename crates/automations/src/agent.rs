use std::{
    collections::BTreeMap,
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
use aow_agents::automation::{AgentAutomation, PromptMode};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

use crate::{AgentKind, Task, store::MAX_RUN_OUTPUT_BYTES};

pub struct CapturedOutput {
    pub truncated: bool,
    pub stderr_tail: Option<String>,
}

pub type SessionReporter = (AgentKind, mpsc::Sender<String>, Arc<AtomicBool>);

pub fn validate_arguments(kind: AgentKind, arguments: &[String]) -> Result<()> {
    kind.validate_arguments(arguments)
}

pub fn command(
    task: &Task,
    agent_environment: &BTreeMap<String, String>,
    directory: &Path,
    session_id: Option<&str>,
) -> Result<Command> {
    validate_arguments(task.input.agent, &task.launch.args)?;
    let mut command = Command::new(&task.launch.executable);
    command
        .args(&task.launch.args)
        .current_dir(directory)
        .envs(&task.launch.environment)
        .envs(agent_environment)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    command
        .args(
            task.input
                .agent
                .automation_arguments(task.input.yolo, session_id)?,
        )
        .stdout(Stdio::piped());
    match task.input.agent.prompt_mode() {
        PromptMode::Stdin => {
            command.stdin(Stdio::piped());
        }
        PromptMode::Argument(flag) => {
            // Keep leading dashes in the prompt from becoming CLI options.
            command
                .arg(format!("{flag}={}", task.input.prompt))
                .stdin(Stdio::null());
        }
    }
    Ok(command)
}

pub async fn capture_stdout(
    reader: impl AsyncRead + Unpin,
    mut file: tokio::fs::File,
    reporter: Option<SessionReporter>,
) -> std::io::Result<CapturedOutput> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let mut oversized = false;
    let mut written = 0_u64;
    let mut truncated = false;
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            break;
        }
        let length = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(buffer.len());
        let chunk = &buffer[..length];
        write_limited(&mut file, chunk, &mut written, &mut truncated).await?;
        if !oversized {
            if line.len() + chunk.len() <= 64 * 1024 {
                line.extend_from_slice(chunk);
            } else {
                oversized = true;
                line.clear();
            }
        }
        let ended = chunk.ends_with(b"\n");
        reader.consume(length);
        if ended {
            report_session(&line, oversized, reporter.as_ref()).await?;
            line.clear();
            oversized = false;
        }
    }
    file.flush().await?;
    Ok(CapturedOutput {
        truncated,
        stderr_tail: None,
    })
}

pub async fn capture_stderr(
    reader: impl AsyncRead + Unpin,
    mut file: tokio::fs::File,
    reporter: Option<SessionReporter>,
) -> std::io::Result<CapturedOutput> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let mut oversized = false;
    let mut tail = Vec::new();
    let mut written = 0_u64;
    let mut truncated = false;
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            break;
        }
        let length = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(buffer.len());
        let chunk = &buffer[..length];
        write_limited(&mut file, chunk, &mut written, &mut truncated).await?;
        tail.extend_from_slice(chunk);
        if tail.len() > 2048 {
            tail.drain(..tail.len() - 2048);
        }
        if !oversized {
            if line.len() + chunk.len() <= 64 * 1024 {
                line.extend_from_slice(chunk);
            } else {
                oversized = true;
                line.clear();
            }
        }
        let ended = chunk.ends_with(b"\n");
        reader.consume(length);
        if ended {
            report_session(&line, oversized, reporter.as_ref()).await?;
            line.clear();
            oversized = false;
        }
    }
    file.flush().await?;
    Ok(CapturedOutput {
        truncated,
        stderr_tail: Some(
            String::from_utf8_lossy(&tail)
                .trim()
                .chars()
                .take(1000)
                .collect(),
        ),
    })
}

async fn report_session(
    line: &[u8],
    oversized: bool,
    reporter: Option<&SessionReporter>,
) -> std::io::Result<()> {
    let Some((agent, sender, sent)) = reporter else {
        return Ok(());
    };
    if oversized {
        return Ok(());
    }
    let Some(session) = agent.session_from_line(line) else {
        return Ok(());
    };
    if sent
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }
    sender
        .send(session)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "session receiver closed"))
}

async fn write_limited(
    file: &mut tokio::fs::File,
    bytes: &[u8],
    written: &mut u64,
    truncated: &mut bool,
) -> std::io::Result<()> {
    let remaining = MAX_RUN_OUTPUT_BYTES.saturating_sub(*written) as usize;
    let take = bytes.len().min(remaining);
    if take > 0 {
        file.write_all(&bytes[..take]).await?;
        *written += take as u64;
    }
    if take < bytes.len() {
        *truncated = true;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn persists_limited_stdout_and_extracts_json_or_human_session_id() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stdio");
        let mut output = vec![b'x'; 100_000];
        output.extend_from_slice(
            b"\n{\"type\":\"item.started\",\"thread_id\":\"wrong\"}\nsession id: native-session\n",
        );
        output.extend_from_slice(&vec![b'x'; MAX_RUN_OUTPUT_BYTES as usize]);
        let (tx, mut rx) = mpsc::channel(1);
        let captured = capture_stdout(
            output.as_slice(),
            tokio::fs::File::from_std(std::fs::File::create(&path).unwrap()),
            Some((AgentKind::Codex, tx, Arc::new(AtomicBool::new(false)))),
        )
        .await
        .unwrap();
        assert_eq!(rx.recv().await.as_deref(), Some("native-session"));
        assert!(rx.recv().await.is_none());
        assert!(captured.truncated);
        assert_eq!(std::fs::metadata(path).unwrap().len(), MAX_RUN_OUTPUT_BYTES);
    }

    #[test]
    fn accepts_json_thread_started_events_for_existing_runs() {
        assert_eq!(
            AgentKind::Codex
                .session_from_line(br#"{"type":"thread.started","thread_id":"native-session"}"#),
            Some("native-session".into())
        );
    }
}
