//! Read-only Git hosting platform providers, independent of their business consumers.

mod adapter;
mod bundled;
mod defaults;
mod description;
mod error;
mod manager;
mod repository_info;
mod request;
mod schema;
mod settings;
mod settings_api;
mod startup;
mod validation;

pub(crate) use error::GitPlatformError;
pub(crate) use request::GitPlatformQuery;
pub(crate) use schema::PUBLIC_PATH as PUBLIC_SCHEMA_PATH;
pub(crate) use settings::GitPlatformProviderManager;
pub(crate) use settings_api::routes;

use adapter::{git, run_adapter};
#[cfg(test)]
use adapter::{read_limited, run_command};
use error::Result;
use request::{CommitLinks, Target};
#[cfg(test)]
use settings::CONFIG;
use settings::{GitPlatformProvider, SCRIPT_LIMIT, Settings};
use validation::{absolute, invalid_json, parse_remote, validate_settings, web_link};

#[cfg(test)]
mod tests;
