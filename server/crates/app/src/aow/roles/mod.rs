//! Reusable roles layer identity and instructions over the existing runtimes.
mod acp;
#[cfg(test)]
mod acp_tests;
mod api;
mod connection;
#[cfg(test)]
mod connection_tests;
mod home;
mod identity;
#[cfg(test)]
mod identity_tests;
mod launch;
mod model;
mod node;
mod runtime;
mod store;
#[cfg(test)]
mod tests;

pub(super) use api::routes;
pub(crate) use launch::RoleSession;
pub(crate) use model::{AgentRole, RoleRuntime};
pub(super) use store::RoleStore;

pub(in crate::aow) use acp::{AcpRoleSession, SESSIONS_FILE};
