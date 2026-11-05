//! Server business API data shared by its application and clients.
pub mod git;
pub mod instance;
pub mod pull_request;
pub mod terminal;

pub use aow_runtime_contract::error::ApiError;

#[cfg(test)]
mod tests;
