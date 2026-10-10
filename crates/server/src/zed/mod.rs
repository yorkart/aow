//! AoW host adapter and HTTP boundary. ACP internals live exclusively in aow-zed.
mod api;
mod host;

pub(crate) use api::routes;
pub(crate) use host::service;
