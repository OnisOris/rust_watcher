use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use graph_query::trace::{build_edge_trace, build_node_trace, build_route_trace};
use serde::Deserialize;

use crate::state::AppStateHandle;

#[derive(Debug, Deserialize)]
pub(crate) struct RouteTraceQuery {
    pub(crate) method: String,
    pub(crate) path: String,
}

pub(crate) async fn trace_node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let Some(node) = graph.nodes.iter().find(|node| node.id == id) else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    (StatusCode::OK, Json(build_node_trace(&graph, node))).into_response()
}

pub(crate) async fn trace_edge(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let edge_id = id.trim_start_matches('/').to_string();
    let Some(edge) = graph.edges.iter().find(|edge| edge.id == edge_id) else {
        return (StatusCode::NOT_FOUND, "edge not found").into_response();
    };
    (StatusCode::OK, Json(build_edge_trace(&graph, edge))).into_response()
}

pub(crate) async fn trace_route_query(
    State(state): State<AppStateHandle>,
    Query(query): Query<RouteTraceQuery>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let requested = graph_core::route_key(&query.method, &query.path).key;
    match graph_query::find_active_endpoint_by_route_key(&graph, &requested) {
        Some(endpoint) => {
            (StatusCode::OK, Json(build_route_trace(&graph, endpoint))).into_response()
        }
        None => (StatusCode::NOT_FOUND, "active route not found").into_response(),
    }
}

pub(crate) async fn trace_route(
    State(state): State<AppStateHandle>,
    AxumPath(route_key): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let requested = route_key.trim_start_matches('/');
    match graph_query::find_active_endpoint_by_route_key(&graph, requested) {
        Some(endpoint) => {
            (StatusCode::OK, Json(build_route_trace(&graph, endpoint))).into_response()
        }
        None => (StatusCode::NOT_FOUND, "active route not found").into_response(),
    }
}
