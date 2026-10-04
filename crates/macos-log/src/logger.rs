use std::{
    ffi::{CStr, CString},
    sync::Arc,
};

use super::os_log::{Handle, LogType, create_log, write_log};

#[derive(Clone)]
pub(super) struct Logger(Arc<Handle>);

impl Logger {
    pub(super) fn new(category: &CStr) -> Self {
        Self(Arc::new(create_log(category)))
    }

    pub(super) fn emit(&self, level: LogType, message: &str) {
        // Bound each os_log dynamic string to avoid its normal message-size
        // truncation. Keep long error chains/backtraces and UTF-8 intact.
        let message = message.replace('\0', "\\0");
        let mut rest = message.trim_end_matches('\n');
        let mut continued = false;
        while !rest.is_empty() {
            let mut end = rest.len().min(768);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let chunk = &rest[..end];
            let text = if continued {
                CString::new(format!("[continued] {chunk}"))
            } else {
                CString::new(chunk)
            }
            .expect("NULs were escaped above");
            write_log(&self.0, level, &text);
            continued = true;
            rest = &rest[end..];
        }
    }
}
