use anyhow::{Context, Result, bail};
use aow_acpd::Options;
use std::path::PathBuf;

fn main() -> Result<()> {
    // New configuration, cache and transcript files are private to the runtime user.
    unsafe {
        libc::umask(0o077);
    }
    #[cfg(target_os = "macos")]
    let unified = aow_macos_log::init_from_env(c"acpd", "aow_acpd=info");
    #[cfg(not(target_os = "macos"))]
    let unified = false;
    if !unified {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "aow_acpd=info".into()),
            )
            .init();
    }
    let result = run();
    #[cfg(target_os = "macos")]
    if let Err(error) = &result {
        aow_macos_log::report_error(&format!("AoW acpd failed: {error:#}"));
    }
    result
}

#[tokio::main]
async fn run() -> Result<()> {
    let mut root = aow_filesystem::default_state_dir();
    let mut socket = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--state-dir" => {
                root = PathBuf::from(args.next().context("--state-dir requires a path")?)
            }
            "--socket" => {
                socket = Some(PathBuf::from(
                    args.next().context("--socket requires a path")?,
                ))
            }
            "-h" | "--help" => {
                println!(
                    "Usage: aow-acpd [--state-dir PATH] [--socket PATH]\nState: --state-dir > AOW_STATE_DIR > $HOME/.aow"
                );
                return Ok(());
            }
            _ => bail!("Unknown acpd option: {arg}"),
        }
    }
    let socket = socket.unwrap_or_else(|| aow_acpd_client::default_socket_path(&root));
    aow_acpd::run_with_shutdown(
        Options {
            state_dir: root,
            socket,
        },
        async {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        },
    )
    .await
}
