//! Scrollback storage, terminal mode/title tracking, and restore snapshots.

mod parser;

use super::*;

pub(super) use parser::TerminalState;
#[cfg(test)]
pub(super) use parser::{MouseEncoding, MouseProtocol};

pub(super) struct OutputState {
    pub(super) scrollback: VecDeque<u8>,
    pub(super) base_offset: u64,
    pub(super) next_offset: u64,
    pub(super) closed: bool,
    pub(super) state_parser: vte::Parser,
    pub(super) terminal: TerminalState,
}

pub(super) struct OutputSnapshot {
    pub(super) offset: u64,
    pub(super) reset: bool,
    pub(super) replay: Vec<u8>,
    pub(super) next_offset: u64,
    pub(super) closed: bool,
    pub(super) restore: Option<String>,
    pub(super) restore_cols: Option<u16>,
    pub(super) restore_rows: Option<u16>,
    pub(super) restore_diagnostics: RestoreDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RestoreDiagnostics {
    pub(super) snapshot_selected: bool,
    pub(super) reason: &'static str,
    pub(super) snapshot_bytes: usize,
    pub(super) snapshot_offset: Option<u64>,
}

#[derive(Debug, Clone)]
pub(super) enum RuntimeEvent {
    Output {
        offset: u64,
        bytes: Bytes,
    },
    OutputClosed,
    Status {
        status: TerminalPaneStatus,
        exit_code: Option<u32>,
    },
    Deleted,
}

impl OutputState {
    pub(super) fn snapshot(&mut self, after: Option<u64>) -> OutputSnapshot {
        self.snapshot_with_vt(after, None, "")
    }

    pub(super) fn snapshot_with_vt(
        &mut self,
        after: Option<u64>,
        vt_snapshot: Option<VtSnapshot>,
        stream_epoch: &str,
    ) -> OutputSnapshot {
        self.snapshot_with_vt_diagnostics(after, vt_snapshot, stream_epoch, "snapshot_not_ready")
    }

    pub(super) fn snapshot_with_vt_diagnostics(
        &mut self,
        after: Option<u64>,
        vt_snapshot: Option<VtSnapshot>,
        stream_epoch: &str,
        snapshot_unavailable_reason: &'static str,
    ) -> OutputSnapshot {
        let resumable =
            after.is_some_and(|offset| (self.base_offset..=self.next_offset).contains(&offset));
        let (vt_snapshot, restore_reason) = if resumable {
            (None, "resume_cursor_used")
        } else {
            match vt_snapshot {
                Some(snapshot) if snapshot.generation != stream_epoch => {
                    (None, "generation_mismatch")
                }
                Some(snapshot)
                    if !(self.base_offset..=self.next_offset)
                        .contains(&snapshot.applied_offset) =>
                {
                    (None, "stale_offset")
                }
                Some(snapshot) => (Some(snapshot), "selected"),
                None => (None, snapshot_unavailable_reason),
            }
        };
        let restore_diagnostics = RestoreDiagnostics {
            snapshot_selected: vt_snapshot.is_some(),
            reason: restore_reason,
            snapshot_bytes: vt_snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.ansi.len()),
            snapshot_offset: vt_snapshot.as_ref().map(|snapshot| snapshot.applied_offset),
        };
        let offset = if resumable {
            after.expect("resumable offsets are present")
        } else if let Some(snapshot) = &vt_snapshot {
            snapshot.applied_offset
        } else {
            self.base_offset
        };
        let replay_start =
            usize::try_from(offset - self.base_offset).expect("scrollback offsets fit in usize");
        OutputSnapshot {
            offset,
            reset: !resumable,
            replay: self.scrollback.make_contiguous()[replay_start..].to_vec(),
            next_offset: self.next_offset,
            closed: self.closed,
            // A complete raw replay already contains every mode transition.
            // Prepending the current state there could change save/restore
            // semantics (notably 1049). Only repair state when truncation has
            // discarded the beginning of the stream.
            restore: vt_snapshot
                .as_ref()
                .map(|snapshot| snapshot.ansi.clone())
                .or_else(|| {
                    (!resumable && self.base_offset > 0).then(|| self.terminal.restore_sequence())
                }),
            restore_cols: vt_snapshot.as_ref().map(|snapshot| snapshot.cols),
            restore_rows: vt_snapshot.as_ref().map(|snapshot| snapshot.rows),
            restore_diagnostics,
        }
    }

    pub(super) fn append(&mut self, bytes: &[u8], limit: usize) -> Result<u64, TerminaldError> {
        let offset = self.next_offset;
        let byte_count = u64::try_from(bytes.len())
            .map_err(|_| TerminaldError::Worker("terminal output length overflow".to_owned()))?;
        self.next_offset = self
            .next_offset
            .checked_add(byte_count)
            .ok_or_else(|| TerminaldError::Worker("terminal output offset overflow".to_owned()))?;

        self.scrollback.extend(bytes);
        let overflow = self.scrollback.len().saturating_sub(limit);
        if overflow != 0 {
            self.scrollback.drain(..overflow);
            self.base_offset = self
                .base_offset
                .checked_add(u64::try_from(overflow).expect("scrollback overflow fits in u64"))
                .ok_or_else(|| {
                    TerminaldError::Worker("terminal scrollback offset overflow".to_owned())
                })?;
        }
        debug_assert_eq!(
            self.next_offset - self.base_offset,
            u64::try_from(self.scrollback.len()).expect("scrollback length fits in u64")
        );
        Ok(offset)
    }
}
