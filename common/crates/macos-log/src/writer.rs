use std::io::{self, Write};

use tracing::{Level, Metadata};
use tracing_subscriber::fmt::MakeWriter;

use super::{logger::Logger, os_log::LogType};

pub(super) struct EventWriter<'a> {
    logger: &'a Logger,
    level: LogType,
    bytes: Vec<u8>,
}

impl<'a> MakeWriter<'a> for Logger {
    type Writer = EventWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        EventWriter {
            logger: self,
            level: LogType::Default,
            bytes: Vec::new(),
        }
    }

    fn make_writer_for(&'a self, metadata: &Metadata<'_>) -> Self::Writer {
        EventWriter {
            logger: self,
            bytes: Vec::new(),
            level: match *metadata.level() {
                Level::ERROR => LogType::Error,
                Level::DEBUG | Level::TRACE => LogType::Debug,
                // Apple's Info is normally memory-only. Keep our existing
                // info/warn diagnostics eligible for persistence using Default.
                Level::INFO | Level::WARN => LogType::Default,
            },
        }
    }
}

impl Write for EventWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for EventWriter<'_> {
    fn drop(&mut self) {
        self.logger
            .emit(self.level, &String::from_utf8_lossy(&self.bytes));
    }
}
