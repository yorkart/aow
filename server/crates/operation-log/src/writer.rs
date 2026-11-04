use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

use super::{Error, MAX_RECORD_BYTES, Record};

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
