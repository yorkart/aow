//! Reusable roles layer identity and instructions over the existing runtimes.
use crate::agents::AgentLaunch;
use crate::error::ServiceError;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
mod profiles;
mod service;
pub(crate) use service::RoleService;
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

pub(crate) use api::routes;
pub(crate) use launch::RoleSession;
pub(crate) use model::{AgentRole, RoleRuntime};
use store::RoleStore;

use acp::{AcpRoleSession, SESSIONS_FILE};
