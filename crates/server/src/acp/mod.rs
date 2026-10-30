//! AoW host adapter and HTTP boundary. ACP internals live exclusively in aow-acp.
mod api;
mod cli;
mod control;
mod host;
mod notifications;

pub(crate) use api::routes;
pub(crate) use cli::routes as cli_routes;
pub(crate) use control::Controls;
pub(crate) use host::service;

mod task_lifecycle;
pub(crate) use api::error as task_error;
pub(crate) use cli::start as start_task;
pub(crate) use task_lifecycle::delete_task_session;
