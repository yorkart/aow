//! Global Markdown requirements, independent of projects until execution.
mod api;
mod context;
mod execution;
mod model;
mod persistence;
mod store;

pub(crate) use api::{cli_routes, routes};
pub(crate) use store::InboxStore;

#[cfg(test)]
mod tests;
