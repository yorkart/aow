use std::path::PathBuf;

pub fn default_socket_path() -> PathBuf {
    if let Some(path) = nonempty_env("AOW_TERMINALD_SOCKET") {
        return PathBuf::from(path);
    }
    if let Some(runtime_dir) = nonempty_env("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir)
            .join("aow-terminald")
            .join("terminald.sock");
    }
    let uid = unsafe { libc::geteuid() };
    PathBuf::from(format!("/tmp/aow-terminald-{uid}/terminald.sock"))
}

fn nonempty_env(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}
