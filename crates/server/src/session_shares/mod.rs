mod api;
mod store;

pub(crate) const PUBLIC_API_PATH: &str = "/api/public/session-shares/{token}";

pub(crate) use api::routes;
pub(crate) use store::SessionShares;

#[cfg(test)]
use crate::AppState;
#[cfg(test)]
use aow_agents::sessions::AgentSessionLocator;
#[cfg(test)]
use axum::{
    Router,
    http::{StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
#[cfg(test)]
use std::{path::Path, time::Duration};
#[cfg(test)]
use store::{CACHE_TTL, SHARES_FILE};

#[cfg(test)]
mod tests;
