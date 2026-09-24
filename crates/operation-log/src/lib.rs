//! Append-only operation history. The reader is independent of the writer and
//! requires neither a database nor an external search process.
//!
//! Cursors are exclusive byte boundaries in hourly UTC files. Files must only
//! be appended to (never copy-truncated). Call blocking IO from a worker thread.

mod reader;
pub use reader::{Cursor, Filter, Page, ReadOptions, Reader};

use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

pub const MAX_RECORD_BYTES: usize = 64 * 1024;
pub const DEFAULT_SCAN_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SCAN_BYTES: usize = 16 * 1024 * 1024;
pub const MIN_SCAN_BYTES: usize = MAX_RECORD_BYTES + 2;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("日志读写失败：{0}")]
    Io(#[from] io::Error),
    #[error("日志格式错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("日志游标无效")]
    InvalidCursor,
    #[error("日志文件已过期或发生变更，请刷新日志")]
    CursorExpired,
    #[error("日志查询参数无效")]
    InvalidOptions,
    #[error("单条日志超过大小限制")]
    RecordTooLarge,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    PartialSuccess,
    Failed,
    Interrupted,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub timestamp: String,
    pub operation_id: String,
    pub boot_id: String,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub event: String,
    pub level: Level,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
}

/// Uses tracing's standard rolling appender. Appends are serialized and flushed
/// before returning; this does not promise durability across power loss.
pub struct Writer {
    appender: Mutex<RollingFileAppender>,
}

impl Writer {
    pub fn open(directory: &Path, retained_files: usize) -> Result<Self, Error> {
        if retained_files == 0 {
            return Err(Error::InvalidOptions);
        }
        // Separate a torn final record from the next record after a restart.
        // Do this before constructing the appender so crossing an hour boundary
        // cannot make us open a file the appender has not created yet.
        let path = directory.join(
            chrono::Utc::now()
                .format("operations.%Y-%m-%d-%H.log")
                .to_string(),
        );
        match std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .open(path)
        {
            Ok(mut file) if file.metadata()?.len() > 0 => {
                file.seek(SeekFrom::End(-1))?;
                let mut last = [0];
                file.read_exact(&mut last)?;
                if last[0] != b'\n' {
                    file.write_all(b"\n")?;
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::HOURLY)
            .filename_prefix("operations")
            .filename_suffix("log")
            .max_log_files(retained_files)
            .build(directory)
            .map_err(io::Error::other)?;
        Ok(Self {
            appender: Mutex::new(appender),
        })
    }

    pub fn append(&self, record: &Record) -> Result<(), Error> {
        let mut bytes = serde_json::to_vec(record)?;
        if bytes.len() + 1 > MAX_RECORD_BYTES {
            return Err(Error::RecordTooLarge);
        }
        bytes.push(b'\n');
        let mut appender = self
            .appender
            .lock()
            .map_err(|_| io::Error::other("log writer poisoned"))?;
        appender.write_all(&bytes)?;
        appender.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
