use super::{ProcessInfo, recognize_process};

#[test]
fn recognizes_native_agents_and_interpreter_entrypoints_only() {
    for (executable, args, expected) in [
        ("/usr/bin/codex", vec!["codex"], Some("codex")),
        ("/usr/bin/codex", vec!["claude"], Some("codex")),
        ("/usr/bin/claude", vec!["traecli"], Some("claude")),
        ("/usr/bin/codex (deleted)", vec!["codex"], Some("codex")),
        ("/opt/traecli", vec!["traecli"], Some("traecli")),
        ("/opt/traecli (deleted)", vec!["traecli"], Some("traecli")),
        (
            "/opt/traex",
            vec!["/home/me/.local/bin/traecli"],
            Some("traecli"),
        ),
        (
            "/home/me/.local/share/claude/versions/2.1.2",
            vec!["2.1.2"],
            Some("claude"),
        ),
        (
            "/usr/bin/node",
            vec![
                "node",
                "/opt/node_modules/@openai/codex/bin/codex.js",
                "hello",
            ],
            Some("codex"),
        ),
        (
            "/usr/bin/node",
            vec![
                "node",
                "--require",
                "setup.js",
                "/opt/node_modules/@anthropic-ai/claude-code/cli.js",
            ],
            Some("claude"),
        ),
        (
            "/usr/bin/python3",
            vec!["python3", "/opt/bin/traecli"],
            Some("traecli"),
        ),
        ("/usr/bin/echo", vec!["echo", "codex"], None),
        (
            "/usr/bin/python3.13",
            vec!["python3.13", "/venv/bin/hermes", "--cli"],
            Some("hermes"),
        ),
        (
            "/Library/Frameworks/Python.framework/Python",
            vec!["python3", "/venv/bin/hermes"],
            Some("hermes"),
        ),
        ("/usr/bin/python3", vec!["python3", "-c", "hermes"], None),
        (
            "/usr/bin/python3",
            vec!["python3", "-m", "unrelated", "hermes"],
            None,
        ),
        (
            "/usr/bin/python3",
            vec!["python3", "/project/worker.py", "/venv/bin/hermes"],
            None,
        ),
        ("/usr/bin/echo", vec!["echo", "hermes"], None),
        (
            "/usr/bin/node",
            vec!["node", "server.js", "/opt/codex"],
            None,
        ),
        ("/usr/bin/node", vec!["node", "-e", "codex"], None),
        (
            "/usr/bin/node",
            vec!["node", "--eval=0", "/opt/codex"],
            None,
        ),
        ("/usr/bin/node", vec!["node", "", "/opt/codex"], None),
        ("/usr/bin/node", vec!["node", "/project/codex.js"], None),
        ("/usr/bin/bash", vec!["bash", "-c", "claude"], None),
        ("/usr/bin/gemini", vec!["gemini"], None),
    ] {
        assert_eq!(
            recognize_process(&ProcessInfo::new(Some(executable), &args)).map(crate::Agent::id),
            expected,
            "{args:?}"
        );
    }
}

#[test]
fn detection_uses_the_registered_agent_identity_for_launch_commands() {
    for agent in crate::KNOWN_AGENTS
        .iter()
        .filter(|agent| agent.detects_processes())
    {
        let definition = agent.definition();
        for command in definition.commands {
            let path = format!("/opt/bin/{command}");
            let detected = recognize_process(&ProcessInfo::new(Some(&path), &[command])).unwrap();
            assert_eq!(detected.id(), definition.id);
            assert_eq!(detected.definition().display_name, definition.display_name);
        }
    }
    assert_eq!(
        crate::Agent::from_id("traecli"),
        Some(crate::Agent::TraeCli)
    );
    assert_eq!(crate::Agent::from_id("trae"), None);
    assert_eq!(crate::Agent::from_id("traex"), None);
    assert_eq!(crate::TRAECLI.commands, &["traecli"]);
    assert_eq!(
        crate::TRAECLI.configuration_env,
        &["TRAECLI_HOME", "TRAE_HOME"]
    );
}

#[test]
fn recognizes_traecli_launch_entrypoints_independently_of_binary_layout() {
    for executable in [
        None,
        Some("/arbitrary/location/worker"),
        Some("/different/location/renamed-binary"),
        Some("/old/location/worker (deleted)"),
    ] {
        for entrypoint in [
            "traecli",
            "/home/me/.local/bin/traecli",
            "/custom/bin/traecli",
        ] {
            assert_eq!(
                recognize_process(&ProcessInfo::new(executable, &[entrypoint])),
                Some(crate::Agent::TraeCli),
                "{executable:?}: {entrypoint}"
            );
        }
    }
}

#[test]
fn does_not_infer_traecli_identity_from_an_unrelated_binary_or_directory() {
    for (executable, args) in [
        (None, vec!["traex"]),
        (Some("/opt/traex"), vec!["traex"]),
        (Some("/opt/traecli/worker"), vec!["worker"]),
        (Some("/arbitrary/location/worker"), vec!["my-agent"]),
        (Some("/usr/bin/node"), vec!["node", "/opt/bin/traex"]),
    ] {
        assert_eq!(
            recognize_process(&ProcessInfo::new(executable, &args)),
            None,
            "{args:?}"
        );
    }
}

#[test]
fn ignores_traecli_and_traex_in_command_arguments() {
    for (executable, args) in [
        (Some("/usr/bin/echo"), vec!["echo", "traex"]),
        (Some("/usr/bin/bash"), vec!["bash", "-c", "traex"]),
        (Some("/usr/bin/node"), vec!["node", "server.js", "traex"]),
        (Some("/usr/bin/echo"), vec!["echo", "traecli"]),
        (Some("/usr/bin/bash"), vec!["bash", "-c", "traecli"]),
        (Some("/usr/bin/node"), vec!["node", "server.js", "traecli"]),
    ] {
        assert_eq!(
            recognize_process(&ProcessInfo::new(executable, &args)),
            None,
            "{args:?}"
        );
    }
}
