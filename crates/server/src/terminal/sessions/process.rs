use std::path::PathBuf;

use aow_agents::sessions::SessionEnvironment;
use aow_protocol::TerminalAgentProcess;

pub(in crate::terminal) fn same_process(process: &TerminalAgentProcess) -> bool {
    aow_process::same_process(process.pid, &process.start_time)
}

pub(in crate::terminal) fn process_environment(
    process: &TerminalAgentProcess,
) -> Option<SessionEnvironment> {
    let bytes = aow_process::environment(process.pid, &process.start_time).ok()?;
    Some(configuration_environment(&bytes))
}

fn configuration_environment(bytes: &[u8]) -> SessionEnvironment {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let (key, value) = std::str::from_utf8(entry).ok()?.split_once('=')?;
            (matches!(key, "HOME" | "PATH")
                || aow_agents::KNOWN_AGENTS
                    .iter()
                    .any(|agent| agent.definition().configuration_env.contains(&key)))
            .then(|| (key.to_owned(), PathBuf::from(value)))
            .filter(|(_, value)| !value.as_os_str().is_empty())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn only_configuration_paths_are_read_from_the_agent_environment() {
        let keys: HashSet<_> = aow_agents::KNOWN_AGENTS
            .iter()
            .flat_map(|agent| agent.definition().configuration_env.iter().copied())
            .collect();
        let mut bytes = b"HOME=/home/user\0PATH=/usr/bin\0SECRET=hidden\0".to_vec();
        for key in &keys {
            bytes.extend_from_slice(format!("{key}=/config/{key}\0").as_bytes());
        }
        let environment = configuration_environment(&bytes);
        assert_eq!(environment.len(), keys.len() + 2);
        for key in keys {
            assert_eq!(environment[key], PathBuf::from(format!("/config/{key}")));
            assert!(configuration_environment(format!("{key}=\0").as_bytes()).is_empty());
        }
        assert!(!environment.contains_key("SECRET"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_process_environment_uses_the_agents_config_and_rejects_stale_identity() {
        use std::process::{Command, Stdio};
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let ready = root.join("environment.ready");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "terminal::sessions::process::tests::native_environment_fixture",
        ]);
        command.env("AOW_NATIVE_ENV_FIXTURE", "1");
        command.env("AOW_NATIVE_ENV_READY", &ready);
        let keys: Vec<_> = aow_agents::KNOWN_AGENTS
            .iter()
            .flat_map(|agent| agent.definition().configuration_env.iter().copied())
            .chain(["HOME", "PATH"])
            .collect();
        for key in &keys {
            command.env(key, root.join(format!("{key} with spaces")));
        }
        let mut child = ChildGuard(
            command
                .env("AOW_TEST_SECRET", "must not be retained")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        // A PID can be observable before the exec has exposed the fixture's
        // environment through /proc. Wait for the child to enter the fixture.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !ready.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "native environment fixture exited before becoming ready"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "native environment fixture did not become ready"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let info = aow_process::info(child.0.id() as i32).unwrap();
        let mut process = TerminalAgentProcess {
            pid: info.pid,
            start_time: info.start_time,
            cwd: root.to_string_lossy().into_owned(),
        };
        assert!(same_process(&process));
        let environment = process_environment(&process).unwrap();
        assert_eq!(environment.len(), keys.len());
        for key in keys {
            assert_eq!(environment[key], root.join(format!("{key} with spaces")));
        }
        assert!(!environment.contains_key("AOW_TEST_SECRET"));
        let original = process.start_time.clone();
        process.start_time = "different start identity".into();
        assert!(process_environment(&process).is_none());
        process.start_time = original;
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        assert!(!same_process(&process));
        assert!(process_environment(&process).is_none());
    }

    #[test]
    #[ignore = "subprocess fixture invoked by native process/session tests"]
    fn native_environment_fixture() {
        if std::env::var_os("AOW_NATIVE_ENV_FIXTURE").is_some() {
            if let Some(ready) = std::env::var_os("AOW_NATIVE_ENV_READY") {
                std::fs::write(ready, b"ready").unwrap();
            }
            std::io::stdin().read_line(&mut String::new()).unwrap();
        }
    }
}
