#[derive(Debug, Default)]
pub(in crate::runtime) struct TerminalState {
    pub(in crate::runtime) title: Option<String>,
    pub(in crate::runtime) application_cursor: bool,
    pub(in crate::runtime) application_keypad: bool,
    pub(in crate::runtime) alternate_screen: bool,
    pub(in crate::runtime) focus_events: bool,
    pub(in crate::runtime) mouse_protocol: MouseProtocol,
    pub(in crate::runtime) mouse_encoding: MouseEncoding,
    pub(in crate::runtime) bracketed_paste: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) enum MouseProtocol {
    #[default]
    None,
    X10,
    Normal,
    Button,
    Any,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime) enum MouseEncoding {
    #[default]
    Default,
    Sgr,
    SgrPixels,
}

impl TerminalState {
    pub(in crate::runtime) fn soft_reset(&mut self) {
        // Match xterm's DECSTR scope for the modes tracked here. A soft reset
        // restores core input modes but deliberately leaves the active screen
        // buffer and mouse service state intact.
        self.application_cursor = false;
        self.application_keypad = false;
        self.focus_events = false;
        self.bracketed_paste = false;
    }

    pub(in crate::runtime) fn hard_reset(&mut self) {
        *self = Self::default();
    }

    pub(in crate::runtime) fn set_private_mode(&mut self, mode: u16, enabled: bool) {
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

    pub(in crate::runtime) fn restore_sequence(&self) -> String {
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
