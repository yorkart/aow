//! Read-only libproc descriptor inspection. These public SDK structures are absent from libc.

use std::{ffi::OsString, io, mem::size_of, os::unix::ffi::OsStringExt};

use crate::OpenFile;

const PROC_PIDFDVNODEPATHINFO: i32 = 2;
const FWRITE: u32 = 2;
const MAX_DESCRIPTORS: usize = 16_384;

#[repr(C)]
struct ProcFileInfo {
    open_flags: u32,
    status: u32,
    offset: i64,
    file_type: i32,
    guard_flags: u32,
}

#[repr(C)]
struct VnodeFdInfoWithPath {
    file: ProcFileInfo,
    vnode: libc::vnode_info_path,
}

pub(crate) fn open_files(pid: i32) -> io::Result<Vec<OpenFile>> {
    let descriptors = descriptors(pid)?;
    let mut files = Vec::new();
    for descriptor in descriptors {
        if descriptor.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            continue;
        }
        let mut info: VnodeFdInfoWithPath = unsafe { std::mem::zeroed() };
        let size = size_of::<VnodeFdInfoWithPath>() as i32;
        let result = unsafe {
            libc::proc_pidfdinfo(
                pid,
                descriptor.proc_fd,
                PROC_PIDFDVNODEPATHINFO,
                (&mut info as *mut VnodeFdInfoWithPath).cast(),
                size,
            )
        };
        if result <= 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(libc::EBADF | libc::ENOENT)) {
                continue;
            }
            return Err(error);
        }
        if result != size {
            return Err(io::Error::other("incomplete descriptor information"));
        }
        let bytes: Vec<u8> = info
            .vnode
            .vip_path
            .iter()
            .flatten()
            .map(|b| *b as u8)
            .collect();
        let Some(end) = bytes.iter().position(|b| *b == 0).filter(|end| *end > 0) else {
            continue;
        };
        files.push(OpenFile {
            path: OsString::from_vec(bytes[..end].to_vec()).into(),
            writable: info.file.open_flags & FWRITE != 0,
        });
    }
    Ok(files)
}

fn descriptors(pid: i32) -> io::Result<Vec<libc::proc_fdinfo>> {
    for _ in 0..4 {
        let bytes =
            unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if bytes <= 0 {
            return Err(io::Error::last_os_error());
        }
        let count = bytes as usize / size_of::<libc::proc_fdinfo>() + 32;
        if count > MAX_DESCRIPTORS {
            return Err(io::Error::other("too many process descriptors"));
        }
        let mut entries = vec![
            libc::proc_fdinfo {
                proc_fd: 0,
                proc_fdtype: 0
            };
            count
        ];
        let capacity = (entries.len() * size_of::<libc::proc_fdinfo>()) as i32;
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDLISTFDS,
                0,
                entries.as_mut_ptr().cast(),
                capacity,
            )
        };
        if bytes <= 0 {
            return Err(io::Error::last_os_error());
        }
        if bytes < capacity {
            if !(bytes as usize).is_multiple_of(size_of::<libc::proc_fdinfo>()) {
                return Err(io::Error::other("incomplete descriptor list"));
            }
            entries.truncate(bytes as usize / size_of::<libc::proc_fdinfo>());
            return Ok(entries);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "descriptor list kept growing",
    ))
}
