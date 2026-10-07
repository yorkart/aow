//! Read-only OS process inspection shared by terminald and the web server.
//! Agent recognition and terminal foreground selection belong to callers.

mod identity;
mod limits;
mod model;
mod platform;

pub use identity::{environment, open_files, same_process};
pub(crate) use limits::{COMMAND_LIMIT, ENVIRONMENT_LIMIT};
pub use model::{OpenFile, ProcessCommand, ProcessInfo};
pub use platform::{command, cwd, info, list_pids};
