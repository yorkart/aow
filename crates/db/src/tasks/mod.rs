//! SQLite storage for task identities, hierarchy, workspace and main-session ownership.
mod store;
#[cfg(test)]
mod tests;
pub use store::{TaskDb, TaskRecord};
