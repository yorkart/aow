//! A stable task lock inode. PID metadata is diagnostic; flock is the arbiter.
//! Children register their process group before exec, while their inherited FD
//! still holds the lock. After a runner crash, a surviving group prevents takeover.
use crate::{
    Store,
    store::{private_dir, try_exclusive, valid_component},
};
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
};
use tokio::process::Command;

struct TaskLock {
    file: File,
}

/// A stable task-local concurrency slot. The slot stays reserved by every
/// process group the runner starts, so a crashed runner cannot immediately
/// allow an orphaned Agent to be exceeded.
pub(crate) struct ConcurrencySlot {
    lock: TaskLock,
}

#[derive(Deserialize)]
struct Owner {
    boot: String,
    run_id: String,
}
#[derive(Deserialize)]
struct Process {
    process_group: i32,
    started_before: u64,
}

impl Store {
    pub(crate) fn concurrency_slot(
        &self,
        task_id: &str,
        max_concurrent_runs: u8,
    ) -> Result<Option<ConcurrencySlot>> {
        valid_component(task_id)?;
        ensure!(
            (1..=10).contains(&max_concurrent_runs),
            "最大同时执行数必须为 1–10"
        );
        for slot in 1..=max_concurrent_runs {
            if let Some(lock) = self.try_task_lock(task_id, &format!("slot-{slot}.lock"))? {
                return Ok(Some(ConcurrencySlot { lock }));
            }
        }
        Ok(None)
    }

    /// Observe registered process groups without ever acquiring a runner slot.
    /// The active journal covers the runner; these records cover orphaned children.
    pub(crate) fn has_occupied_concurrency_slot(&self, task_id: &str) -> Result<bool> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id);
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("slot-") && name.ends_with(".lock"))
            {
                continue;
            }
            let file = match OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            ensure!(file.metadata()?.is_file(), "任务锁不是普通文件");
            let mut contents = String::new();
            file.take(64 * 1024 + 1).read_to_string(&mut contents)?;
            ensure!(contents.len() <= 64 * 1024, "任务锁信息过大");
            let mut lines = contents
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'));
            let Some(first) = lines.next() else {
                continue;
            };
            // Diagnostic metadata can be empty, partial, or stale during a handoff.
            let Ok(owner) = serde_json::from_str::<Owner>(first) else {
                continue;
            };
            if valid_component(&owner.run_id).is_err() || owner.boot != boot_identity()? {
                continue;
            }
            match self.read_run(task_id, &owner.run_id) {
                Ok(Some(run)) if run.finished_at.is_some() => continue,
                Ok(_) => {}
                Err(error) if crate::store::is_not_found(&error) => {}
                Err(error) => return Err(error),
            }
            for line in lines {
                if let Ok(process) = serde_json::from_str::<Process>(line)
                    && group_alive(&process)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn try_task_lock(&self, task_id: &str, name: &str) -> Result<Option<TaskLock>> {
        valid_component(task_id)?;
        let directory = self.root.join("runs").join(task_id);
        private_dir(&directory)?;
        self.try_task_lock_path(&directory.join(name))
    }

    fn try_task_lock_path(&self, path: &std::path::Path) -> Result<Option<TaskLock>> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        if !try_exclusive(&file)? {
            return Ok(None);
        }
        ensure!(file.metadata()?.len() <= 64 * 1024, "任务锁信息过大");
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let mut lines = contents.lines();
        if let Some(first) = lines.next()
            && let Ok(owner) = serde_json::from_str::<Owner>(first)
            && owner.boot == boot_identity()?
        {
            for line in lines {
                // A partial final registration cannot have reached exec.
                if let Ok(process) = serde_json::from_str::<Process>(line)
                    && group_alive(&process)?
                {
                    return Ok(None);
                }
            }
        }
        Ok(Some(TaskLock { file }))
    }
}

impl TaskLock {
    fn begin(&mut self, run_id: &str) -> Result<()> {
        self.file.set_len(0)?;
        serde_json::to_writer(
            &mut self.file,
            &serde_json::json!({
                "runner_pid": std::process::id(), "run_id": run_id, "boot": boot_identity()?
            }),
        )?;
        self.file.write_all(b"\n")?;
        self.file.sync_data()?;
        Ok(())
    }

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
    pub fn begin(&mut self, run_id: &str) -> Result<()> {
        self.lock.begin(run_id)
    }

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
fn boot_identity() -> Result<String> {
    Ok(fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .into())
}
#[cfg(target_os = "macos")]
fn boot_identity() -> Result<String> {
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

fn group_alive(process: &Process) -> Result<bool> {
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
