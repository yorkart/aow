use crate::{
    DEFAULT_SCAN_BYTES, Error, Level, MAX_RECORD_BYTES, MAX_SCAN_BYTES, MIN_SCAN_BYTES, Record,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const BLOCK_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    pub file: String,
    /// Exclusive upper boundary. Normally the start of the last scanned line.
    /// May be inside an oversized/incomplete line, which the reader discards.
    pub offset: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filter {
    pub query: Option<String>,
    pub operation_id: Option<String>,
    pub kind: Option<String>,
    pub source: Option<String>,
    pub project_id: Option<String>,
    pub level: Option<Level>,
}

impl Filter {
    fn matches(&self, record: &Record) -> bool {
        self.operation_id
            .as_ref()
            .is_none_or(|value| value == &record.operation_id)
            && self.kind.as_ref().is_none_or(|value| value == &record.kind)
            && self
                .source
                .as_ref()
                .is_none_or(|value| value == &record.source)
            && self
                .project_id
                .as_ref()
                .is_none_or(|value| record.project_id.as_ref() == Some(value))
            && self.level.is_none_or(|value| value == record.level)
            && self.query.as_ref().is_none_or(|value| {
                record.message.contains(value)
                    || record.title.contains(value)
                    || record
                        .resource
                        .as_ref()
                        .is_some_and(|resource| resource.contains(value))
                    || record.operation_id.contains(value)
            })
    }
}

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub cursor: Option<Cursor>,
    pub filter: Filter,
    pub limit: usize,
    pub scan_bytes: usize,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            cursor: None,
            filter: Filter::default(),
            limit: 50,
            scan_bytes: DEFAULT_SCAN_BYTES,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Page {
    pub items: Vec<Record>,
    pub next_cursor: Option<Cursor>,
    pub budget_exhausted: bool,
    pub scanned_bytes: usize,
}

#[derive(Clone)]
pub struct Reader {
    directory: PathBuf,
}

impl Reader {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn read(&self, options: &ReadOptions) -> Result<Page, Error> {
        if !(1..=200).contains(&options.limit)
            || !(MIN_SCAN_BYTES..=MAX_SCAN_BYTES).contains(&options.scan_bytes)
            || options
                .filter
                .query
                .as_ref()
                .is_some_and(|query| query.len() > 1024)
        {
            return Err(Error::InvalidOptions);
        }
        if options
            .cursor
            .as_ref()
            .is_some_and(|cursor| !log_file_name(&cursor.file))
        {
            return Err(Error::InvalidCursor);
        }
        let mut files = self.files()?;
        // Names encode UTC rotation order. New files cannot move an old cursor.
        files.sort_unstable_by(|a, b| b.cmp(a));
        if let Some(cursor) = &options.cursor {
            let index = files
                .iter()
                .position(|file| file == &cursor.file)
                .ok_or(Error::CursorExpired)?;
            files.drain(..index);
        }
        let mut page = Page::default();
        for (index, name) in files.iter().enumerate() {
            let mut file = match File::open(self.directory.join(name)) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(Error::CursorExpired);
                }
                Err(error) => return Err(error.into()),
            };
            let length = file.metadata()?.len();
            let mut position = if index == 0 {
                options
                    .cursor
                    .as_ref()
                    .map_or(length, |cursor| cursor.offset)
            } else {
                length
            };
            if position > length {
                return Err(Error::CursorExpired);
            }
            let mut resume = position;
            let mut collecting = false;
            let mut oversized = false;
            let mut line = Vec::new();
            while position > 0 && page.scanned_bytes < options.scan_bytes {
                let count = position
                    .min(BLOCK_BYTES as u64)
                    .min((options.scan_bytes - page.scanned_bytes) as u64)
                    as usize;
                let start = position - count as u64;
                file.seek(SeekFrom::Start(start))?;
                let mut block = vec![0; count];
                file.read_exact(&mut block)?;
                page.scanned_bytes += count;
                for (byte_index, byte) in block.into_iter().enumerate().rev() {
                    if byte == b'\n' {
                        if collecting && !oversized {
                            accept(&mut line, options, &mut page);
                        }
                        line.clear();
                        oversized = false;
                        collecting = true;
                        resume = start + byte_index as u64 + 1;
                        if page.items.len() == options.limit {
                            page.next_cursor = Some(Cursor {
                                file: name.clone(),
                                offset: resume,
                            });
                            return Ok(page);
                        }
                    } else if collecting && !oversized {
                        if line.len() < MAX_RECORD_BYTES {
                            line.push(byte);
                        } else {
                            line.clear();
                            oversized = true;
                        }
                    }
                }
                position = start;
            }
            if position > 0 {
                // Re-read a bounded unfinished valid line on the next page. The
                // minimum budget exceeds the largest valid record, guaranteeing
                // progress. Oversized/torn lines can safely resume mid-line.
                let offset = if collecting && !oversized {
                    resume
                } else {
                    position
                };
                page.next_cursor = Some(Cursor {
                    file: name.clone(),
                    offset,
                });
                page.budget_exhausted = true;
                return Ok(page);
            }
            if collecting && !oversized {
                accept(&mut line, options, &mut page);
            }
            if page.items.len() == options.limit || page.scanned_bytes == options.scan_bytes {
                if let Some(older) = files.get(index + 1) {
                    let metadata = fs::metadata(self.directory.join(older)).map_err(|error| {
                        if error.kind() == std::io::ErrorKind::NotFound {
                            Error::CursorExpired
                        } else {
                            error.into()
                        }
                    })?;
                    page.next_cursor = Some(Cursor {
                        file: older.clone(),
                        offset: metadata.len(),
                    });
                    page.budget_exhausted = page.items.len() < options.limit;
                }
                return Ok(page);
            }
        }
        Ok(page)
    }

    fn files(&self) -> Result<Vec<String>, Error> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            // Do not read symlinks or unrelated files even with a crafted cursor.
            if entry.file_type()?.is_file()
                && let Some(name) = entry.file_name().to_str()
                && log_file_name(name)
            {
                files.push(name.to_owned());
            }
        }
        Ok(files)
    }
}

fn accept(line: &mut [u8], options: &ReadOptions, page: &mut Page) {
    line.reverse();
    if let Ok(record) = serde_json::from_slice::<Record>(line)
        && options.filter.matches(&record)
    {
        page.items.push(record);
    }
}

fn log_file_name(name: &str) -> bool {
    let Some(date) = name
        .strip_prefix("operations.")
        .and_then(|value| value.strip_suffix(".log"))
    else {
        return false;
    };
    date.len() == 13
        && date.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7 | 10) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
        && Path::new(name).components().count() == 1
}
