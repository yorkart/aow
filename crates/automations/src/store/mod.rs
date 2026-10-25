mod files;
mod history;
mod locks;
mod result;
mod runs;
mod session_roots;
mod tasks;
mod writer;

use std::path::PathBuf;

use anyhow::Result;
use aow_config::ConfigStore;

pub use files::{atomic_write, new_run_id, private_dir, valid_component};
pub use history::RunCursor;
pub(crate) use locks::{is_not_found, try_exclusive};
pub use runs::MAX_RUN_OUTPUT_BYTES;
pub use writer::RunWriter;

use locks::{FileLock, try_shared};

#[derive(Clone)]
pub struct Store {
    pub state_dir: PathBuf,
    pub root: PathBuf,
    pub config_dir: PathBuf,
    config: ConfigStore,
}
impl Store {
    pub fn new(state_dir: PathBuf) -> Result<Self> {
        ConfigStore::initialize(&state_dir)?;
        let store = Self::open(state_dir)?;
        private_dir(&store.root)?;
        private_dir(&store.root.join("runs"))?;
        Ok(store)
    }

    /// Resolve paths without creating state, changing permissions, or running cleanup.
    pub fn open(state_dir: PathBuf) -> Result<Self> {
        let state_dir = if state_dir.is_absolute() {
            state_dir
        } else {
            std::env::current_dir()?.join(state_dir)
        };
        let root = state_dir.join("state/automations");
        let config = ConfigStore::open(&state_dir)?;
        let config_dir = state_dir.clone();
        Ok(Self {
            state_dir,
            root,
            config_dir,
            config,
        })
    }
}

#[cfg(test)]
mod tests;
