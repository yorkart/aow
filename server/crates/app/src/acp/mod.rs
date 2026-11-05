//! Business API and notification wiring around the remote runtime client.
mod api;
mod cli;
mod client;
mod control;
mod notifications;

pub(crate) use api::error as task_error;
pub(crate) use api::routes;
pub(crate) use cli::routes as cli_routes;
pub(crate) use cli::start as start_task;
pub(crate) use client::service;
pub(crate) use control::Controls;
