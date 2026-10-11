//! AoW host adapter and HTTP boundary. ACP internals live exclusively in aow-acp.
mod api;
mod control;
mod host;
mod notifications;

pub(crate) use api::routes;
pub(crate) use control::Controls;
pub(crate) use host::service;
