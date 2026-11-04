//! Private local configuration files with atomic writes and process-safe locking.

mod layout;
mod paths;
mod storage;
mod store;

pub use paths::default_state_dir;
pub use store::ConfigStore;

pub mod environment;
