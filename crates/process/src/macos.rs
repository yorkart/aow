use std::{
    ffi::OsString,
    io,
    mem::{size_of, size_of_val},
    os::unix::ffi::OsStringExt,
    path::PathBuf,
};

use crate::{
    COMMAND_LIMIT, ENVIRONMENT_LIMIT, ProcessCommand, ProcessInfo, procargs::parse_procargs,
};

// Public libproc selector, not currently exported by the libc crate.
const PROC_UID_ONLY: u32 = 4;

pub fn list_pids() -> io::Result<Vec<i32>> {
    // Only the service user's processes can belong to its terminal panes.
    // Retry a growing snapshot, but do not spin indefinitely during fork churn.
    for _ in 0..4 {
        let bytes =
            unsafe { libc::proc_listpids(PROC_UID_ONLY, libc::getuid(), std::ptr::null_mut(), 0) };
        if bytes < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pids = vec![0i32; bytes as usize / size_of::<i32>() + 32];
        let capacity = i32::try_from(pids.len() * size_of::<i32>())
            .map_err(|_| invalid_data("process list is too large"))?;
        let bytes = unsafe {
            libc::proc_listpids(
                PROC_UID_ONLY,
                libc::getuid(),
                pids.as_mut_ptr().cast(),
                capacity,
            )
        };
        if bytes < 0 {
            return Err(io::Error::last_os_error());
        }
        if bytes < capacity {
            if !(bytes as usize).is_multiple_of(size_of::<i32>()) {
                return Err(invalid_data("incomplete process list"));
            }
            pids.truncate(bytes as usize / size_of::<i32>());
            pids.retain(|pid| *pid > 0);
            return Ok(pids);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "process list kept growing",
    ))
}

fn bsd_info(pid: i32) -> io::Result<libc::proc_bsdinfo> {
    valid_pid(pid)?;
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    let size = size_of_val(&info) as i32;
    let result = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    check_size(result, size)?;
    Ok(info)
}

pub fn info(pid: i32) -> io::Result<ProcessInfo> {
    let bsd = bsd_info(pid)?;
    let session = unsafe { libc::getsid(pid) };
    if session < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ProcessInfo {
        pid,
        parent: bsd.pbi_ppid as i32,
        group: bsd.pbi_pgid as i32,
        session,
        // Darwin uses NODEV (-1); Linux and the shared selector use zero.
        tty: if bsd.e_tdev == u32::MAX {
            0
        } else {
            bsd.e_tdev as i32
        },
        foreground: bsd.e_tpgid as i32,
        state: match bsd.pbi_status {
            libc::SIDL => 'I',
            libc::SRUN => 'R',
            libc::SSLEEP => 'S',
            libc::SSTOP => 'T',
            libc::SZOMB => 'Z',
            _ => '?',
        },
        start_time: start_identity(&bsd),
    })
}

pub fn command(pid: i32) -> ProcessCommand {
    let executable = executable(pid).ok();
    let arguments = arguments_and_environment(pid)
        .and_then(|bytes| {
            let (arguments, _) = parse_procargs(&bytes)?;
            Ok(arguments[..arguments.len().min(COMMAND_LIMIT as usize)].to_vec())
        })
        .unwrap_or_default();
    ProcessCommand {
        executable,
        arguments,
    }
}

fn executable(pid: i32) -> io::Result<PathBuf> {
    valid_pid(pid)?;
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let result = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    if result <= 0 {
        return Err(io::Error::last_os_error());
    }
    nul_path(&path)
}

pub fn cwd(pid: i32) -> io::Result<PathBuf> {
    valid_pid(pid)?;
    let mut info = unsafe { std::mem::zeroed::<libc::proc_vnodepathinfo>() };
    let size = size_of_val(&info) as i32;
    let result = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    check_size(result, size)?;
    let path: Vec<_> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .map(|byte| *byte as u8)
        .collect();
    nul_path(&path)
}

pub(super) fn start_time(pid: i32) -> io::Result<String> {
    bsd_info(pid).map(|info| start_identity(&info))
}

fn start_identity(info: &libc::proc_bsdinfo) -> String {
    format!("{}.{:06}", info.pbi_start_tvsec, info.pbi_start_tvusec)
}

pub(super) fn environment(pid: i32) -> io::Result<Vec<u8>> {
    let bytes = arguments_and_environment(pid)?;
    let (_, environment) = parse_procargs(&bytes)?;
    let environment = environment.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "process environment was omitted by macOS",
        )
    })?;
    Ok(environment.to_vec())
}

fn arguments_and_environment(pid: i32) -> io::Result<Vec<u8>> {
    valid_pid(pid)?;
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX, 0];
    let mut argmax = 0i32;
    let mut size = size_of_val(&argmax);
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut argmax as *mut i32).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if size != size_of_val(&argmax) || argmax <= 0 || argmax as u64 > ENVIRONMENT_LIMIT {
        return Err(invalid_data("invalid process argument limit"));
    }
    // Darwin can return success for a too-small buffer. Reserve the complete
    // argument space plus the argc header, never a truncated suffix that could
    // confuse argument strings with configuration environment entries.
    let mut bytes = vec![0u8; argmax as usize + size_of::<i32>()];
    size = bytes.len();
    mib[1] = libc::KERN_PROCARGS2;
    mib[2] = pid;
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if size > bytes.len() {
        return Err(invalid_data("incomplete process arguments"));
    }
    bytes.truncate(size);
    Ok(bytes)
}

fn valid_pid(pid: i32) -> io::Result<()> {
    if pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid process id",
        ));
    }
    Ok(())
}

fn check_size(result: i32, expected: i32) -> io::Result<()> {
    if result <= 0 {
        Err(io::Error::last_os_error())
    } else if result != expected {
        Err(invalid_data("incomplete process information"))
    } else {
        Ok(())
    }
}

fn nul_path(bytes: &[u8]) -> io::Result<PathBuf> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .filter(|end| *end > 0)
        .ok_or_else(|| invalid_data("invalid process path"))?;
    Ok(OsString::from_vec(bytes[..end].to_vec()).into())
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
