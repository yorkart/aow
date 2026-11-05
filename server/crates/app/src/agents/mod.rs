use crate::error::ServiceError;
use crate::validation;
pub(crate) use aow_host_contract::agents::AgentLaunch;
use aow_host_contract::agents::AgentRegistration;
use aow_host_contract::agents::AgentType;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
mod api;
mod service;
pub(crate) mod sessions;
mod storage;
pub(crate) use api::routes;
pub(crate) use service::AgentService;
use storage::RegistryState;
#[cfg(test)]
pub(crate) use storage::StoredAgent;
#[cfg(not(test))]
use storage::StoredAgent;
mod launch;
mod order;
mod registry;
#[cfg(test)]
mod tests;

pub(crate) use order::ReorderAgentsRequest;

pub(crate) use registry::{AgentsQuery, RegisterAgentRequest};

#[cfg(test)]
mod testing;

#[cfg(test)]
pub(crate) use testing::resolve_on_node;
