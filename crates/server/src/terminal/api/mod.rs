use super::*;
use axum::{
    Router,
    routing::{get, post, put},
};

mod attachment;
mod clipboard;
mod origin;
mod rebuild;
mod tabs;

#[cfg(test)]
pub(crate) use origin::validate_request_origin;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/terminals",
            get(tabs::list_terminals).post(tabs::create_terminal),
        )
        .route("/api/terminals/order", put(tabs::reorder_terminals))
        .route("/api/terminals/agents", get(tabs::list_terminal_agents))
        .route("/api/terminals/task-stops", get(notifications::events))
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}/agent-sessions",
            get(sessions::list),
        )
        .route(
            "/api/terminals/{tab_id}",
            get(tabs::get_terminal)
                .patch(tabs::update_terminal)
                .delete(tabs::delete_terminal),
        )
        .route("/api/terminals/{tab_id}/split", post(tabs::split_terminal))
        .route(
            "/api/terminals/{tab_id}/rebuild",
            post(rebuild::rebuild_terminal),
        )
        .route(
            "/api/terminals/{tab_id}/layout",
            put(tabs::update_terminal_layout),
        )
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}",
            axum::routing::delete(tabs::delete_terminal_pane),
        )
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}/ws",
            get(attachment::attach_terminal_pane),
        )
        .route(
            "/api/terminals/{tab_id}/panes/{pane_id}/clipboard-images",
            post(clipboard::upload_terminal_clipboard_image),
        )
}
