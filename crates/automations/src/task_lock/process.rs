#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::{Result, ensure};
#[cfg(target_os = "linux")]
use std::fs;
use std::os::fd::AsRawFd;
use tokio::process::Command;

use super::{ConcurrencySlot, Process, TaskLock};

impl TaskLock {
    fn register(&self, command: &mut Command) {
        let fd = self.file.as_raw_fd();
        // No allocations or non-async-signal-safe syscalls after fork. The FD is
        // CLOEXEC, so the kernel lock is inherited only until this record is complete.
        unsafe {
            command.pre_exec(move || {
                let mut time = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                };
                if libc::clock_gettime(process_clock(), &mut time) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let mut buffer = [0u8; 128];
                let mut n = 0;
                append(&mut buffer, &mut n, b"{\"process_group\":");
                number(&mut buffer, &mut n, libc::getpid() as u64);
                append(&mut buffer, &mut n, b",\"started_before\":");
                number(
                    &mut buffer,
                    &mut n,
                    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64,
                );
                append(&mut buffer, &mut n, b"}\n");
                let mut written = 0;
                while written < n {
                    let count = libc::write(fd, buffer[written..n].as_ptr().cast(), n - written);
                    if count < 0 {
                        let error = std::io::Error::last_os_error();
                        if error.kind() == std::io::ErrorKind::Interrupted {
                            continue;
                        }
                        return Err(error);
                    }
                    if count == 0 {
                        return Err(std::io::Error::from_raw_os_error(libc::EIO));
                    }
                    written += count as usize;
                }
                Ok(())
            });
        }
    }
}

impl ConcurrencySlot {
    pub fn register(&self, command: &mut Command) {
        self.lock.register(command);
    }
}

// Deliberately close rather than LOCK_UN: a forked child may still be registering.
fn append(buffer: &mut [u8], n: &mut usize, value: &[u8]) {
    buffer[*n..*n + value.len()].copy_from_slice(value);
    *n += value.len();
}
fn number(buffer: &mut [u8], n: &mut usize, mut value: u64) {
    let mut digits = [0u8; 20];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    append(buffer, n, &digits[start..]);
}
#[cfg(target_os = "linux")]
fn process_clock() -> libc::clockid_t {
    libc::CLOCK_BOOTTIME
}
#[cfg(target_os = "macos")]
fn process_clock() -> libc::clockid_t {
    libc::CLOCK_REALTIME
}

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
        "无法读取系统启动时间"
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
        .context("进程状态无效")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    ensure!(fields.len() > 19, "进程状态不完整");
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    ensure!(ticks > 0, "无法读取进程时钟频率");
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
    ensure!(result == size, "进程状态不完整");
    Ok(Some(ProcessInfo {
        group: info.pbi_pgid as i32,
        started: info.pbi_start_tvsec * 1_000_000_000 + info.pbi_start_tvusec * 1000,
        zombie: info.pbi_status == libc::SZOMB,
    }))
}

pub(super) fn group_alive(process: &Process) -> Result<bool> {
    let group = process.process_group;
    ensure!(group > 1, "任务锁中的进程组无效");
    if unsafe { libc::kill(-group, 0) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(error.into()); // EPERM is not proof of death.
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
        ensure!(size >= 0, "无法读取进程组");
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
        ensure!(bytes >= 0, "无法读取进程组");
        if (bytes as usize) < capacity {
            pids.truncate(bytes as usize / 4);
            return Ok(pids);
        }
    }
}
