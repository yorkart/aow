use std::{
    ffi::CStr,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use crate::HttpError;

pub(crate) static PROCESS_HOME: LazyLock<PathBuf> = LazyLock::new(process_home);
pub(super) const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

pub(super) fn decode_absolute(path: &str) -> Result<PathBuf, HttpError> {
    // Axum's Path extractor has already percent-decoded and UTF-8 validated the value.
    // Decoding it again would turn a literal filename such as `%20.txt` into a space.
    let path = Path::new("/").join(path.trim_start_matches('/'));
    Ok(path)
}

#[cfg(unix)]
fn process_home() -> PathBuf {
    // SAFETY: Called once through LazyLock; pw_dir is copied before returning.
    unsafe {
        let account = libc::getpwuid(libc::geteuid());
        if !account.is_null() && !(*account).pw_dir.is_null() {
            return PathBuf::from(
                CStr::from_ptr((*account).pw_dir)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    PathBuf::from("/")
}

#[cfg(not(unix))]
fn process_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub(crate) fn encode_absolute(path: &str) -> String {
    path.split('/')
        .map(|component| utf8_percent_encode(component, PATH_SEGMENT_ENCODE_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}
