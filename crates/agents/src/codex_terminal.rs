//! Opt managed terminals into embedded Codex only when the selected CLI supports it.

use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

use crate::codex_command::{TERMINAL_ENV, flags};

const HELP_LIMIT: usize = 64 * 1024;
const HELP_TIMEOUT: Duration = Duration::from_secs(2);

/// Prepare both new and rebuilt runtimes, preserving unsupported/custom invocations.
pub async fn prepare(
    executable: &str,
    pane_id: &str,
    cwd: &Path,
    args: &mut Vec<String>,
    environment: &mut BTreeMap<String, String>,
) {
    environment.remove(TERMINAL_ENV);
    let borrowed: Vec<_> = args.iter().map(String::as_str).collect();
    let flags = flags(&borrowed);
    if !flags.interactive
        || flags.remote
        || !supports_no_daemon(executable, cwd, environment, HELP_TIMEOUT).await
    {
        return;
    }
    if flags.no_daemon.is_none() {
        let position = args
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(args.len());
        args.insert(position, "--no-daemon".into());
    }
    environment.insert(TERMINAL_ENV.into(), pane_id.into());
}

async fn supports_no_daemon(
    executable: &str,
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
) -> bool {
    // Probe the selected executable, with its launch environment, at each launch.
    // Do not pass profile arguments: these may contain prompts or non-TUI commands.
    let Ok(mut child) = tokio::process::Command::new(executable)
        .arg("--help")
        .current_dir(cwd)
        .envs(environment)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    else {
        return false;
    };
    let Some(stdout) = child.stdout.take() else {
        return false;
    };
    tokio::time::timeout(timeout, async {
        let mut bytes = Vec::new();
        stdout
            .take(HELP_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        if bytes.len() > HELP_LIMIT || !child.wait().await.ok()?.success() {
            return None;
        }
        let help = std::str::from_utf8(&bytes).ok()?;
        Some(help.lines().any(|line| {
            line.trim_start()
                .strip_prefix("--no-daemon")
                .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(char::is_whitespace))
        }))
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn executable(directory: &Path, body: &str) -> String {
        let path = directory.join("selected-codex");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn older_clis_and_failed_probes_preserve_launch_and_resume_arguments() {
        let directory = tempfile::tempdir().unwrap();
        for body in [
            "printf '%s\\n' 'Codex CLI 0.120.0' '  --no-alt-screen'",
            "printf '%s\\n' '  --no-daemon'; exit 1",
            "printf '%s\\n' '  --no-daemon-other'",
        ] {
            let executable = executable(directory.path(), body);
            for original in [vec![], vec!["resume".into(), "session".into()]] {
                let mut args = original.clone();
                let mut environment = BTreeMap::from([(TERMINAL_ENV.into(), "stale".into())]);
                prepare(
                    &executable,
                    "pane",
                    directory.path(),
                    &mut args,
                    &mut environment,
                )
                .await;
                assert_eq!(args, original);
                assert!(!environment.contains_key(TERMINAL_ENV));
            }
        }
    }

    #[tokio::test]
    async fn ineligible_invocations_are_not_probed_or_modified() {
        let directory = tempfile::tempdir().unwrap();
        let executable = executable(
            directory.path(),
            "touch probed; printf '%s\\n' '  --no-daemon'",
        );
        for original in [
            vec!["exec", "task"],
            vec!["e", "task"],
            vec!["agents"],
            vec!["--profile", "work", "agents"],
            vec!["app-server"],
            vec!["--remote=unix:///tmp/codex.sock"],
            vec!["--unknown-option"],
        ] {
            let original: Vec<String> = original.into_iter().map(String::from).collect();
            let mut args = original.clone();
            let mut environment = BTreeMap::from([(TERMINAL_ENV.into(), "stale".into())]);
            prepare(
                &executable,
                "pane",
                directory.path(),
                &mut args,
                &mut environment,
            )
            .await;
            assert_eq!(args, original);
            assert!(!environment.contains_key(TERMINAL_ENV));
        }
        assert!(!directory.path().join("probed").exists());
    }

    #[tokio::test]
    async fn probe_uses_selected_executable_environment_and_bounds_time_and_output() {
        let directory = tempfile::tempdir().unwrap();
        let environment = BTreeMap::from([("PROBE_OPTION".into(), "--no-daemon".into())]);
        let selected = executable(
            directory.path(),
            "[ \"$#\" = 1 ] && [ \"$1\" = --help ] || exit 1\nprintf '%s\\n' \"  $PROBE_OPTION\"",
        );
        assert!(supports_no_daemon(&selected, directory.path(), &environment, HELP_TIMEOUT).await);
        executable(directory.path(), "exec sleep 30");
        assert!(
            !supports_no_daemon(
                &selected,
                directory.path(),
                &environment,
                Duration::from_millis(50)
            )
            .await
        );
        executable(
            directory.path(),
            "printf '%s\\n' '  --no-daemon'; exec head -c 70000 /dev/zero",
        );
        assert!(!supports_no_daemon(&selected, directory.path(), &environment, HELP_TIMEOUT).await);
        assert!(
            !supports_no_daemon(
                "/missing/codex",
                directory.path(),
                &environment,
                HELP_TIMEOUT
            )
            .await
        );
    }
}
