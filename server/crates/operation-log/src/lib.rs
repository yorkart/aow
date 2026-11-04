//! Append-only operation history. The reader is independent of the writer and
//! requires neither a database nor an external search process.
//!
//! Cursors are exclusive byte boundaries in hourly UTC files. Files must only
//! be appended to (never copy-truncated). Call blocking IO from a worker thread.

mod error;
mod limits;
mod model;
mod reader;
mod writer;

pub use error::Error;
pub use limits::{DEFAULT_SCAN_BYTES, MAX_RECORD_BYTES, MAX_SCAN_BYTES, MIN_SCAN_BYTES};
pub use model::{Level, Outcome, Record};
pub use reader::{Cursor, Filter, Page, ReadOptions, Reader};
pub use writer::Writer;

#[cfg(test)]
mod tests;
