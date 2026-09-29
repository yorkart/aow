//! Lightweight capture and user-defined task states. No state implies execution.
mod api;
mod context;
mod execution;
mod inbox;
mod persistence;
mod store;

pub(crate) use api::routes;
pub(crate) use store::TaskStore;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod storage_tests;
