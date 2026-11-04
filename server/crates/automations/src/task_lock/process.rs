use super::Process;
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::{Result, ensure};
#[cfg(target_os = "linux")]
use std::fs;

#[cfg(target_os = "linux")]
pub(super) fn boot_identity() -> Result<String> {
    Ok(fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .into())
}
#[cfg(target_os = "macos")]
pub(super) fn boot_identity() -> Result<String> {
    let mut boot = [0u8; 128];
    let mut size = boot.len();
    ensure!(
        unsafe {
            libc::sysctlbyname(
                c"kern.bootsessionuuid".as_ptr(),
                boot.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } == 0,
        aow_i18n::message("automations.unable_to_read_system_boot_time", &[])
    );
    Ok(String::from_utf8(boot[..size].to_vec())?
        .trim_end_matches('\0')
        .to_owned())
}

struct ProcessInfo {
    group: i32,
    started: u64,
    zombie: bool,
}
#[cfg(target_os = "linux")]
fn process_info(pid: i32) -> Result<Option<ProcessInfo>> {
    let text = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(text) => text,
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOENT | libc::ESRCH)) => {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let fields = text
        .rsplit_once(") ")
        .context(aow_i18n::text("automations.invalid_process_state"))?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    ensure!(
        fields.len() > 19,
        aow_i18n::message("automations.incomplete_process_state", &[])
    );
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    ensure!(
        ticks > 0,
        aow_i18n::message("automations.unable_to_read_process_clock_frequency", &[])
    );
    Ok(Some(ProcessInfo {
        group: fields[2].parse()?,
        started: fields[19].parse::<u64>()? * (1_000_000_000 / ticks as u64),
        zombie: fields[0] == "Z" || fields[0] == "X",
    }))
}
#[cfg(target_os = "macos")]
fn process_info(pid: i32) -> Result<Option<ProcessInfo>> {
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = std::mem::size_of_val(&info) as i32;
    let result = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(libc::ESRCH | libc::ENOENT)) {
            return Ok(None);
        }
        return Err(error.into());
    }
    ensure!(
        result == size,
        aow_i18n::message("automations.incomplete_process_state", &[])
    );
    Ok(Some(ProcessInfo {
        group: info.pbi_pgid as i32,
        started: info.pbi_start_tvsec * 1_000_000_000 + info.pbi_start_tvusec * 1000,
        zombie: info.pbi_status == libc::SZOMB,
    }))
}

pub(super) fn group_alive(process: &Process) -> Result<bool> {
    let group = process.process_group;
    ensure!(
        group > 1,
        aow_i18n::message("automations.invalid_process_group_in_task_lock", &[])
    );
    if unsafe { libc::kill(-group, 0) } != 0 {
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => return Ok(false),
            // macOS also returns EPERM when the group contains only zombies.
            // Inspect its members before deciding; unreadable processes still
            // fail closed instead of releasing a potentially occupied slot.
            #[cfg(target_os = "macos")]
            Some(libc::EPERM) => {}
            _ => return Err(error.into()),
        }
    }
    if let Some(info) = process_info(group)? {
        // A reused PGID has a newer leader. With a missing leader, an extant
        // group still reserves the original PGID until its final member exits.
        if info.started > process.started_before || info.group != group {
            return Ok(false);
        }
        if !info.zombie {
            return Ok(true);
        }
    }
    // Ignore unreaped zombies, but retain ownership for living descendants.
    for pid in group_pids(group)? {
        if let Some(info) = process_info(pid)?
            && info.group == group
            && !info.zombie
        {
            return Ok(true);
        }
    }
    Ok(false)
}
#[cfg(target_os = "linux")]
fn group_pids(_: i32) -> Result<Vec<i32>> {
    Ok(fs::read_dir("/proc")?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .collect())
}
#[cfg(target_os = "macos")]
fn group_pids(group: i32) -> Result<Vec<i32>> {
    const PROC_PGRP_ONLY: u32 = 2;
    loop {
        let size =
            unsafe { libc::proc_listpids(PROC_PGRP_ONLY, group as u32, std::ptr::null_mut(), 0) };
        ensure!(
            size >= 0,
            aow_i18n::message("automations.unable_to_read_process_group", &[])
        );
        let mut pids = vec![0i32; size as usize / 4 + 32];
        let capacity = pids.len() * 4;
        let bytes = unsafe {
            libc::proc_listpids(
                PROC_PGRP_ONLY,
                group as u32,
                pids.as_mut_ptr().cast(),
                capacity as i32,
            )
        };
        ensure!(
            bytes >= 0,
            aow_i18n::message("automations.unable_to_read_process_group", &[])
        );
        if (bytes as usize) < capacity {
            pids.truncate(bytes as usize / 4);
            return Ok(pids);
        }
    }
}
