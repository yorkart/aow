//! Private local configuration files with atomic writes and process-safe locking.

mod layout;
mod storage;
mod store;

pub use store::ConfigStore;
