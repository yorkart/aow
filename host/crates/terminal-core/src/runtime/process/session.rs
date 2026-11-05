//! Operating-system session membership used by runtime process cleanup.

#[cfg(target_os = "macos")]
pub(in crate::runtime) fn macos_kill_session_members(session_id: i32, start_time: Option<&str>) {
    let Some(start_time) = start_time.filter(|_| session_id > 1) else {
        return;
    };
    // A completed session can disappear before this cleanup runs. Never act
    // on a new session that happens to reuse the old leader's PID.
    match aow_process::info(session_id) {
        Ok(leader) if leader.start_time != start_time => return,
        Err(error) if !matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) => return,
        _ => {}
    }
    let pids = match aow_process::list_pids() {
        Ok(pids) => pids,
        Err(error) => {
            tracing::warn!(%error, session_id, "failed to enumerate terminal session for cleanup");
            return;
        }
    };
    let members: Vec<_> = pids
        .into_iter()
        .filter_map(|pid| aow_process::info(pid).ok())
        .filter(|info| info.pid > 1 && info.session == session_id)
        .collect();
    for member in members {
        // Revalidate both identity and membership immediately before signaling.
        if aow_process::info(member.pid).is_ok_and(|current| {
            current.start_time == member.start_time && current.session == session_id
        }) {
            unsafe {
                libc::kill(member.pid, libc::SIGKILL);
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub(in crate::runtime) fn linux_session_members(session_id: i32) -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
                return false;
            };
            stat.rsplit_once(')')
                .and_then(|(_, fields)| fields.split_whitespace().nth(3))
                .and_then(|session| session.parse::<i32>().ok())
                == Some(session_id)
        })
        .collect()
}
