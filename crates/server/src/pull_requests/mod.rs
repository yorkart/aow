//! Provider-independent review API. Scripts speak the versioned JSON protocol in
//! frontend/src/features/pr/review-providers.md; platform-specific behavior lives in adapters.

mod adapter;
mod api;
mod error;
mod issues;
mod manager;
mod repository_info;
mod request;
mod settings;
mod settings_api;
mod validation;

pub(crate) use api::{my_pull_request_detail, my_pull_request_diff, my_pull_requests};
pub(crate) use error::PullRequestError;
pub(crate) use settings::ProviderManager;
pub(crate) use settings_api::routes;

#[cfg(test)]
use adapter::{OUTPUT_LIMIT, read_limited, run_command};
use adapter::{git, run_adapter};
use error::Result;
pub(crate) use request::ReviewQuery;
use request::{CommitLinks, DiffQuery, PullRequestListQuery, Target};
#[cfg(test)]
use settings::{CONFIG, Document};
#[cfg(test)]
use settings::{GITHUB_SCRIPT, upgrade_bundled_scripts};
use settings::{Provider, SCRIPT_LIMIT, Settings};
use validation::{
    absolute, decode, invalid_json, managed_script_path, parse_remote, positive, safe_path,
    validate_settings, web_link,
};

#[cfg(test)]
mod tests;
