use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
};

use crate::{COMMAND_LIMIT, ENVIRONMENT_LIMIT, ProcessCommand, ProcessInfo};

pub fn list_pids() -> io::Result<Vec<i32>> {
    Ok(fs::read_dir("/proc")?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect())
}

pub fn info(pid: i32) -> io::Result<ProcessInfo> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    parse_stat(pid, &stat)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid process stat"))
}

pub fn command(pid: i32) -> ProcessCommand {
    let executable = fs::read_link(format!("/proc/{pid}/exe")).ok();
    let mut arguments = Vec::new();
    if let Ok(file) = fs::File::open(format!("/proc/{pid}/cmdline")) {
        let _ = file.take(COMMAND_LIMIT).read_to_end(&mut arguments);
    }
    ProcessCommand {
        executable,
        arguments,
    }
}

pub fn cwd(pid: i32) -> io::Result<PathBuf> {
    fs::read_link(format!("/proc/{pid}/cwd"))
}

pub(crate) fn start_time(pid: i32) -> io::Result<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process start time"))
}

pub(crate) fn environment(pid: i32) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(format!("/proc/{pid}/environ"))?
        .take(ENVIRONMENT_LIMIT)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(crate) fn open_files(pid: i32) -> io::Result<Vec<crate::OpenFile>> {
    let mut files = Vec::new();
    for (index, entry) in fs::read_dir(format!("/proc/{pid}/fd"))?.enumerate() {
        if index >= 16_384 {
            return Err(io::Error::other("too many process descriptors"));
        }
        let entry = entry?;
        let read = || -> io::Result<Option<crate::OpenFile>> {
            let path = fs::read_link(entry.path())?;
            if !path.is_absolute() {
                return Ok(None);
            }
            let metadata = fs::read_to_string(
                PathBuf::from(format!("/proc/{pid}/fdinfo")).join(entry.file_name()),
            )?;
            let writable = descriptor_writable(&metadata).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing descriptor flags")
            })?;
            Ok(Some(crate::OpenFile { path, writable }))
        };
        match read() {
            Ok(Some(file)) => files.push(file),
            Ok(None) => {}
            // Descriptors can close between the directory snapshot and readlink.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(files)
}

fn descriptor_writable(metadata: &str) -> Option<bool> {
    let flags = metadata
        .lines()
        .find_map(|line| line.strip_prefix("flags:"))?;
    let flags = u32::from_str_radix(flags.trim(), 8).ok()?;
    // Linux O_ACCMODE and O_PATH, including when these parsers are tested on macOS.
    Some(matches!(flags & 0o3, 1 | 2) && flags & 0o10000000 == 0)
}

fn parse_stat(pid: i32, stat: &str) -> Option<ProcessInfo> {
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    Some(ProcessInfo {
        pid,
        state: fields.next()?.chars().next()?,
        parent: fields.next()?.parse().ok()?,
        group: fields.next()?.parse().ok()?,
        session: fields.next()?.parse().ok()?,
        tty: fields.next()?.parse().ok()?,
        foreground: fields.next()?.parse().ok()?,
        start_time: fields.nth(13)?.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_flags_distinguish_readers_and_writers() {
        assert_eq!(descriptor_writable("flags:\t02102002\n"), Some(true));
        assert_eq!(descriptor_writable("flags:\t0100001\n"), Some(true));
        assert_eq!(descriptor_writable("flags:\t0100000\n"), Some(false));
        assert_eq!(descriptor_writable("flags:\t010000002\n"), Some(false));
        assert_eq!(descriptor_writable("pos:\t0\n"), None);
    }

    #[test]
    fn parses_stat_with_spaces_and_parentheses_in_process_name() {
        let row = parse_stat(
            42,
            "42 (shell (test)) S 10 42 10 34816 42 0 0 0 0 0 0 0 0 0 0 0 0 0 123",
        )
        .unwrap();
        assert_eq!(row.start_time, "123");
        assert_eq!(
            (row.parent, row.group, row.session, row.tty, row.foreground),
            (10, 42, 10, 34816, 42),
        );
        assert!(parse_stat(42, "42 (gone)").is_none());
    }
}
