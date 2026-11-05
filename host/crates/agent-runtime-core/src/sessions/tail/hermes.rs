use super::*;
use crate::sessions::hermes as store;
use aow_db::hermes::HermesDb;

const RECORD_BUDGET: usize = 512;

pub(super) struct SqliteTail {
    cursor: i64,
    metadata: Metadata,
    parser: Box<dyn TaskStopParser>,
    pub modified: SystemTime,
}

impl SqliteTail {
    pub fn from_eof(locator: &AgentSessionLocator) -> Result<Self, SnapshotError> {
        let mut db = HermesDb::open(&locator.transcript_path)?;
        let reader = db.read()?;
        let cursor = reader.watermark()?;
        let metadata = std::fs::metadata(&locator.transcript_path)?;
        let last = reader.last_message_timestamp(&locator.session_id)?;
        let modified = last
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| SystemTime::from(store::timestamp(value)))
            .unwrap_or(SystemTime::UNIX_EPOCH);
        Ok(Self {
            cursor,
            metadata,
            modified,
            parser: crate::Agent::Hermes
                .session_tracking()
                .unwrap()
                .task_stop_parser(),
        })
    }

    pub fn poll(
        &mut self,
        locator: &AgentSessionLocator,
    ) -> Result<Vec<TaskStopped>, SnapshotError> {
        validate_locator(locator)?;
        let metadata = std::fs::metadata(&locator.transcript_path)?;
        let mut db = HermesDb::open(&locator.transcript_path)?;
        let reader = db.read()?;
        let latest = reader.watermark()?;
        if replaced(&self.metadata, &metadata) || latest < self.cursor {
            *self = Self::from_eof(locator)?;
            return Ok(Vec::new());
        }
        if latest == self.cursor {
            return Ok(Vec::new());
        }
        if let Some(records) = store::compacted_records(&reader, &locator.session_id)? {
            // Reconstructed copies retain their original row IDs, including
            // completions archived before the next poll. Bound delivery after
            // reconstruction so a large copied context cannot replay old stops.
            let records: Vec<_> = records
                .into_iter()
                .filter(|record| record["id"].as_i64().is_some_and(|id| id > self.cursor))
                .take(RECORD_BUDGET)
                .collect();
            return Ok(self.consume(locator, records, latest));
        }
        let records = reader
            .tail_messages(&locator.session_id, self.cursor, RECORD_BUDGET)?
            .into_iter()
            .map(store::record)
            .collect();
        Ok(self.consume(locator, records, latest))
    }

    fn consume(
        &mut self,
        locator: &AgentSessionLocator,
        records: Vec<serde_json::Value>,
        latest: i64,
    ) -> Vec<TaskStopped> {
        let count = records.len();
        let mut events = Vec::new();
        for record in records {
            self.cursor = record["id"].as_i64().unwrap_or(self.cursor);
            // WAL commits need not change state.db's mtime. Track activity of
            // this session, not writes by unrelated sessions sharing the DB.
            self.modified = SystemTime::now();
            // A compression continuation keeps the same logical conversation.
            // The database admits only its lineage, including messages committed
            // before the terminal's next session-identity refresh.
            let session_id = record["session_id"].as_str().unwrap_or(&locator.session_id);
            if let Some(event) = self.parser.consume(session_id, &record) {
                events.push(event);
            }
        }
        if count < RECORD_BUDGET {
            self.cursor = latest;
        }
        events
    }
}
