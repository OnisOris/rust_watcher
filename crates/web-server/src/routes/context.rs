use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use graph_query::context_pack::{
    build_edge_context_pack, build_node_context_pack, build_route_context_pack,
    build_trace_context_pack,
};

use crate::routes::trace::RouteTraceQuery;
use crate::state::AppStateHandle;

pub(crate) async fn context_node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let Some(node) = graph.nodes.iter().find(|node| node.id == id) else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_node_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            node,
        )),
    )
        .into_response()
}

pub(crate) async fn context_edge(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let edge_id = id.trim_start_matches('/').to_string();
    let Some(edge) = graph.edges.iter().find(|edge| edge.id == edge_id) else {
        return (StatusCode::NOT_FOUND, "edge not found").into_response();
    };
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_edge_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            edge,
        )),
    )
        .into_response()
}

pub(crate) async fn context_route_query(
    State(state): State<AppStateHandle>,
    Query(query): Query<RouteTraceQuery>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let requested = graph_core::route_key(&query.method, &query.path).key;
    let Some(endpoint) = graph_query::find_active_endpoint_by_route_key(&graph, &requested) else {
        return (StatusCode::NOT_FOUND, "active route not found").into_response();
    };
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_route_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            endpoint,
        )),
    )
        .into_response()
}

pub(crate) async fn context_trace(
    State(state): State<AppStateHandle>,
    Json(trace): Json<graph_core::TraceExplanation>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_trace_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            &trace,
        )),
    )
        .into_response()
}
