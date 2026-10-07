#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
#[cfg(target_os = "linux")]
use linux as selected;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_files;
#[cfg(target_os = "macos")]
use macos as selected;
#[cfg(any(target_os = "macos", test))]
mod procargs;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use unsupported as selected;

#[cfg(target_os = "macos")]
pub(super) use macos_files::open_files;
#[cfg(not(target_os = "macos"))]
pub(super) use selected::open_files;
pub use selected::{command, cwd, info, list_pids};
pub(super) use selected::{environment, start_time};
