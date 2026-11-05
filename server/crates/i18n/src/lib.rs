//! Product-owned language resources backed by the common locale engine.
mod resources;
pub use aow_i18n_core::{
    Language, current_language, http, scope, spawn, spawn_blocking, with_language,
};
pub use resources::{message, text};
