use axum::routing::{get, post, put};
use axum::Router;

use crate::state::AppStateHandle;

pub(crate) mod context;
pub(crate) mod diagnostics;
pub(crate) mod editor;
pub(crate) mod graph;
pub(crate) mod health;
pub(crate) mod layout;
pub(crate) mod project;
pub(crate) mod trace;
pub(crate) mod views;
pub(crate) mod websocket;

pub(crate) fn router() -> Router<AppStateHandle> {
    Router::new()
        .route("/api/health", get(health::health))
        .route("/api/status", get(health::status))
        .route("/api/graph/snapshot", get(graph::snapshot))
        .route("/api/diagnostics", get(diagnostics::diagnostics))
        .route(
            "/api/layout",
            get(layout::layout_get)
                .post(layout::layout_save)
                .delete(layout::layout_clear),
        )
        .route("/api/layout/node", post(layout::layout_save_node))
        .route(
            "/api/views",
            get(views::views_get).post(views::views_create),
        )
        .route(
            "/api/views/{id}",
            put(views::views_update).delete(views::views_delete),
        )
        .route("/api/node/{id}", get(graph::node))
        .route("/api/node/{id}/details", get(graph::node_details))
        .route("/api/trace/node/{id}", get(trace::trace_node))
        .route("/api/trace/edge/{*id}", get(trace::trace_edge))
        .route("/api/trace/route", get(trace::trace_route_query))
        .route("/api/trace/route/by-path", get(trace::trace_route_query))
        .route("/api/trace/route/{*route_key}", get(trace::trace_route))
        .route("/api/context/node/{id}", get(context::context_node))
        .route("/api/context/edge/{*id}", get(context::context_edge))
        .route("/api/context/route", get(context::context_route_query))
        .route("/api/context/trace", post(context::context_trace))
        .route("/api/search", get(graph::search))
        .route("/api/focus", post(graph::focus))
        .route("/api/editor/open", post(editor::open_in_editor))
        .route("/api/project/open", post(project::open_project))
        .route("/ws", get(websocket::ws_handler))
}
