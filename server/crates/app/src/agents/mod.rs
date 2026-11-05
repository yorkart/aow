use crate::error::{ServiceError, service_response};
use crate::{AppState, HttpError, validation};
pub(crate) use aow_runtime_contract::agents::AgentLaunch;
use aow_runtime_contract::agents::AgentRegistration;
use aow_runtime_contract::agents::AgentType;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
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
