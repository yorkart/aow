use crate::{ProcessCommand, ProcessInfo};
use std::{io, path::PathBuf};

fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process inspection is unavailable",
    ))
}

pub fn list_pids() -> io::Result<Vec<i32>> {
    unsupported()
}
pub fn info(_: i32) -> io::Result<ProcessInfo> {
    unsupported()
}
pub fn command(_: i32) -> ProcessCommand {
    ProcessCommand::default()
}
pub fn cwd(_: i32) -> io::Result<PathBuf> {
    unsupported()
}
pub(super) fn start_time(_: i32) -> io::Result<String> {
    unsupported()
}
pub(super) fn environment(_: i32) -> io::Result<Vec<u8>> {
    unsupported()
}
