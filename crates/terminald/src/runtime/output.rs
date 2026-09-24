//! Scrollback storage, terminal mode/title tracking, and restore snapshots.

use super::*;

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

#[derive(Debug, Default)]
pub(super) struct TerminalState {
    pub(super) title: Option<String>,
    pub(super) application_cursor: bool,
    pub(super) application_keypad: bool,
    pub(super) alternate_screen: bool,
    pub(super) focus_events: bool,
    pub(super) mouse_protocol: MouseProtocol,
    pub(super) mouse_encoding: MouseEncoding,
    pub(super) bracketed_paste: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseProtocol {
    #[default]
    None,
    X10,
    Normal,
    Button,
    Any,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseEncoding {
    #[default]
    Default,
    Sgr,
    SgrPixels,
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

impl TerminalState {
    pub(super) fn soft_reset(&mut self) {
        // Match xterm's DECSTR scope for the modes tracked here. A soft reset
        // restores core input modes but deliberately leaves the active screen
        // buffer and mouse service state intact.
        self.application_cursor = false;
        self.application_keypad = false;
        self.focus_events = false;
        self.bracketed_paste = false;
    }

    pub(super) fn hard_reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn set_private_mode(&mut self, mode: u16, enabled: bool) {
        match mode {
            1 => self.application_cursor = enabled,
            9 => {
                self.mouse_protocol = if enabled {
                    MouseProtocol::X10
                } else {
                    MouseProtocol::None
                }
            }
            47 | 1047 | 1049 => self.alternate_screen = enabled,
            66 => self.application_keypad = enabled,
            1000 => {
                self.mouse_protocol = if enabled {
                    MouseProtocol::Normal
                } else {
                    MouseProtocol::None
                }
            }
            1002 => {
                self.mouse_protocol = if enabled {
                    MouseProtocol::Button
                } else {
                    MouseProtocol::None
                }
            }
            1003 => {
                self.mouse_protocol = if enabled {
                    MouseProtocol::Any
                } else {
                    MouseProtocol::None
                }
            }
            1004 => self.focus_events = enabled,
            1006 => {
                self.mouse_encoding = if enabled {
                    MouseEncoding::Sgr
                } else {
                    MouseEncoding::Default
                }
            }
            1016 => {
                self.mouse_encoding = if enabled {
                    MouseEncoding::SgrPixels
                } else {
                    MouseEncoding::Default
                }
            }
            2004 => self.bracketed_paste = enabled,
            _ => {}
        }
    }

    pub(super) fn restore_sequence(&self) -> String {
        let mut restore = String::new();
        for (enabled, mode) in [
            (self.application_cursor, 1),
            // Always use 47 to restore an existing alternate-screen image.
            // 1047/1049 may clear or save/restore cursor state on reattach.
            (self.alternate_screen, 47),
            (self.application_keypad, 66),
            (self.focus_events, 1004),
            (self.bracketed_paste, 2004),
        ] {
            if enabled {
                use std::fmt::Write as _;
                let _ = write!(restore, "\u{1b}[?{mode}h");
            }
        }
        let mouse_protocol = match self.mouse_protocol {
            MouseProtocol::None => None,
            MouseProtocol::X10 => Some(9),
            MouseProtocol::Normal => Some(1000),
            MouseProtocol::Button => Some(1002),
            MouseProtocol::Any => Some(1003),
        };
        let mouse_encoding = match self.mouse_encoding {
            MouseEncoding::Default => None,
            MouseEncoding::Sgr => Some(1006),
            MouseEncoding::SgrPixels => Some(1016),
        };
        for mode in mouse_protocol.into_iter().chain(mouse_encoding) {
            use std::fmt::Write as _;
            let _ = write!(restore, "\u{1b}[?{mode}h");
        }
        restore
    }
}

impl vte::Perform for TerminalState {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.len() < 2 || !matches!(params[0], b"0" | b"2") {
            return;
        }
        // vte uses a fixed OSC buffer (std feature disabled). Rejoin the title's
        // semicolons; only the first field is the OSC command number.
        let bytes = params[1..].join(&b';');
        let text = String::from_utf8_lossy(&bytes);
        let title: String = text
            .chars()
            .filter(|ch| {
                !ch.is_control()
                    && !matches!(*ch,
                '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}' | '\u{feff}')
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(240)
            .collect();
        self.title = (!title.is_empty()).then_some(title);
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if !ignore && intermediates == b"!" && action == 'p' {
            self.soft_reset();
            return;
        }
        if ignore || intermediates != b"?" || !matches!(action, 'h' | 'l') {
            return;
        }
        let enabled = action == 'h';
        for param in params {
            if let Some(mode) = param.first() {
                self.set_private_mode(*mode, enabled);
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore || !intermediates.is_empty() {
            return;
        }
        match byte {
            b'=' => self.application_keypad = true,
            b'>' => self.application_keypad = false,
            b'c' => self.hard_reset(),
            _ => {}
        }
    }
}
