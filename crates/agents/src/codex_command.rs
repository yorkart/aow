//! Interpret only the CLI options needed to opt into local descriptor discovery.

pub(crate) const TERMINAL_ENV: &str = "AOW_CODEX_TERMINAL";

pub(crate) struct Flags {
    pub no_daemon: Option<usize>,
    pub remote: bool,
    pub interactive: bool,
}

pub(crate) fn flags(arguments: &[&str]) -> Flags {
    let mut flags = Flags {
        no_daemon: None,
        remote: false,
        interactive: true,
    };
    let mut args = arguments.iter().copied().enumerate();
    let mut positional = false;
    while let Some((index, arg)) = args.next() {
        match arg {
            "--" => break,
            "--no-daemon" => flags.no_daemon = Some(index),
            "--remote" => {
                flags.remote = true;
                args.next();
            }
            value if value.starts_with("--remote=") => flags.remote = true,
            "-c"
            | "--config"
            | "-m"
            | "--model"
            | "-p"
            | "--profile"
            | "-C"
            | "--cd"
            | "-s"
            | "--sandbox"
            | "-a"
            | "--ask-for-approval"
            | "-i"
            | "--image"
            | "--enable"
            | "--disable"
            | "--add-dir"
            | "--local-provider"
            | "--remote-auth-token-env" => {
                if args.next().is_none() {
                    flags.interactive = false;
                }
            }
            "--oss"
            | "--strict-config"
            | "--approve-for-me"
            | "--search"
            | "--no-alt-screen"
            | "--worktree"
            | "--last"
            | "--all"
            | "--dangerously-bypass-approvals-and-sandbox"
            | "--dangerously-bypass-hook-trust" => {}
            value if value.starts_with("--") && value.contains('=') => {
                let (option, _) = value.split_once('=').unwrap();
                if !matches!(
                    option,
                    "--config"
                        | "--model"
                        | "--profile"
                        | "--cd"
                        | "--sandbox"
                        | "--ask-for-approval"
                        | "--image"
                        | "--enable"
                        | "--disable"
                        | "--add-dir"
                        | "--local-provider"
                        | "--remote-auth-token-env"
                ) {
                    flags.interactive = false;
                }
            }
            value if value.starts_with('-') => flags.interactive = false,
            value if !positional => {
                positional = true;
                if matches!(
                    value,
                    "exec"
                        | "e"
                        | "review"
                        | "app-server"
                        | "exec-server"
                        | "agents"
                        | "queue"
                        | "login"
                        | "logout"
                        | "mcp"
                        | "plugin"
                        | "remote-control"
                        | "app"
                        | "completion"
                        | "update"
                        | "doctor"
                        | "sandbox"
                        | "debug"
                        | "apply"
                        | "a"
                        | "archive"
                        | "delete"
                        | "migrate-rollouts"
                        | "unarchive"
                        | "cloud"
                        | "features"
                        | "help"
                ) {
                    flags.interactive = false;
                }
            }
            _ => {}
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_actual_local_tui_flags_enable_discovery() {
        for args in [
            vec!["--no-daemon"],
            vec!["--no-daemon", "resume", "abc"],
            vec!["-c", "features.daemon_auto_start=false", "--no-daemon"],
        ] {
            let parsed = flags(&args);
            assert!(parsed.interactive && parsed.no_daemon.is_some() && !parsed.remote);
        }
        for args in [
            vec![],
            vec!["--", "--no-daemon"],
            vec!["-c", "--no-daemon"],
            vec!["--model", "--no-daemon"],
            vec!["--no-daemon", "--remote", "unix:///tmp/server"],
            vec!["--remote=unix:///tmp/server", "--no-daemon"],
            vec!["exec", "--no-daemon"],
            vec!["--no-daemon", "--unknown-option"],
        ] {
            let parsed = flags(&args);
            assert!(
                !(parsed.interactive && parsed.no_daemon.is_some() && !parsed.remote),
                "{args:?}"
            );
        }
    }
}
