//! Node-local OS process inspection and optional bounded subprocess execution.
//! Agent recognition and terminal foreground selection belong to callers.

mod identity;
mod limits;
mod model;
mod platform;

pub use identity::{environment, open_files, same_process};
pub(crate) use limits::{COMMAND_LIMIT, ENVIRONMENT_LIMIT};
pub use model::{OpenFile, ProcessCommand, ProcessInfo};
pub use platform::{command, cwd, info, list_pids};

#[cfg(feature = "execution")]
mod execution;
#[cfg(feature = "execution")]
pub use execution::{ProcessResult, ProcessSpec, run_once};
