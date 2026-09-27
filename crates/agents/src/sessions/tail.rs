//! Bounded, forward-only reading of native task-stop records. New readers
//! start at the current file EOF or SQLite message watermark.

mod hermes;

use std::{
    fs::{File, Metadata},
    io::{Read, Seek, SeekFrom},
    time::SystemTime,
};

use super::{
    AgentSessionLocator,
    snapshot::{SnapshotError, validate_locator},
    tracking::{AgentSessionTracker, TaskStopParser},
};

pub use super::tracking::TaskStopped;

const READ_BUDGET: u64 = 1024 * 1024;
const MAX_LINE: usize = 1024 * 1024;

pub struct SessionTail {
    pub locator: AgentSessionLocator,
    pub modified: SystemTime,
    source: TailSource,
}

enum TailSource {
    Jsonl(JsonlTail),
    Hermes(hermes::SqliteTail),
}

impl SessionTail {
    pub fn from_eof(locator: AgentSessionLocator) -> Result<Self, SnapshotError> {
        validate_locator(&locator)?;
        let source = if locator.agent == "hermes" {
            TailSource::Hermes(hermes::SqliteTail::from_eof(&locator)?)
        } else {
            TailSource::Jsonl(JsonlTail::from_eof(locator.clone())?)
        };
        let modified = match &source {
            TailSource::Jsonl(tail) => tail.modified,
            TailSource::Hermes(tail) => tail.modified,
        };
        Ok(Self {
            locator,
            modified,
            source,
        })
    }

    pub fn poll(&mut self) -> Result<Vec<TaskStopped>, SnapshotError> {
        let (events, modified) = match &mut self.source {
            TailSource::Jsonl(tail) => (tail.poll()?, tail.modified),
            TailSource::Hermes(tail) => (tail.poll(&self.locator)?, tail.modified),
        };
        self.modified = modified;
        Ok(events)
    }

    /// Refresh file activity before budget eviction. SQLite activity is tracked
    /// per session by its reader; a shared DB's mtime is not that session's age.
    pub fn refresh_modified(&mut self) {
        if matches!(self.source, TailSource::Jsonl(_))
            && let Ok(modified) = std::fs::metadata(&self.locator.transcript_path)
                .and_then(|metadata| metadata.modified())
        {
            self.modified = modified;
        }
    }
}

struct JsonlTail {
    pub locator: AgentSessionLocator,
    parser: Box<dyn TaskStopParser>,
    file: File,
    offset: u64,
    partial: Vec<u8>,
    discard_line: bool,
    pub modified: SystemTime,
}

impl JsonlTail {
    pub fn from_eof(locator: AgentSessionLocator) -> Result<Self, SnapshotError> {
        validate_locator(&locator)?;
        let tracker = crate::Agent::from_id(locator.agent)
            .and_then(crate::Agent::session_tracking)
            .ok_or_else(|| {
                SnapshotError::Invalid("agent does not support session tracking".into())
            })?;
        let mut file = File::open(&locator.transcript_path)?;
        let metadata = file.metadata()?;
        let offset = metadata.len();
        // A record which began before registration is historical, even if the
        // writer has not appended its newline yet.
        let discard_line = if offset > 0 {
            file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            file.read_exact(&mut last)?;
            last[0] != b'\n'
        } else {
            false
        };
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            locator,
            parser: tracker.task_stop_parser(),
            file,
            offset,
            partial: Vec::new(),
            discard_line,
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        })
    }

    pub fn poll(&mut self) -> Result<Vec<TaskStopped>, SnapshotError> {
        let path_metadata = std::fs::metadata(&self.locator.transcript_path)?;
        let metadata = self.file.metadata()?;
        if replaced(&metadata, &path_metadata) || metadata.len() < self.offset {
            // Replacement/truncation is a new attachment, never a history replay.
            *self = Self::from_eof(self.locator.clone())?;
            return Ok(Vec::new());
        }
        self.modified = metadata.modified().unwrap_or(self.modified);
        let mut bytes = Vec::new();
        (&mut self.file).take(READ_BUDGET).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        let mut events = Vec::new();
        for byte in bytes {
            if byte == b'\n' {
                if !self.discard_line {
                    match serde_json::from_slice(&self.partial) {
                        Ok(record) => {
                            if let Some(event) =
                                self.parser.consume(&self.locator.session_id, &record)
                            {
                                events.push(event);
                            }
                        }
                        // A skipped record may have contained a new turn or reply.
                        Err(_) => self.parser.reset(),
                    }
                }
                self.partial.clear();
                self.discard_line = false;
            } else if !self.discard_line {
                if self.partial.len() == MAX_LINE {
                    self.partial.clear();
                    self.discard_line = true;
                    self.parser.reset();
                } else {
                    self.partial.push(byte);
                }
            }
        }
        Ok(events)
    }
}

#[cfg(unix)]
fn replaced(before: &Metadata, after: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    (before.dev(), before.ino()) != (after.dev(), after.ino())
}

#[cfg(not(unix))]
fn replaced(before: &Metadata, after: &Metadata) -> bool {
    before.created().ok() != after.created().ok()
}

#[cfg(test)]
mod tests;
