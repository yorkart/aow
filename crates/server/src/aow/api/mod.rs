use super::*;
use axum::{
    Router,
    routing::{delete, get, post},
};

mod agents;
mod projects;
mod sessions;
mod settings;
mod worktrees;

pub(crate) use sessions::resolve_session_locator;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .merge(configuration::routes())
        .merge(settings::routes())
        .merge(projects::routes())
        .merge(worktrees::routes())
        .merge(agents::routes())
        .merge(sessions::routes())
}
