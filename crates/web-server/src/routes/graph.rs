use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use graph_builder::filter_snapshot;
use graph_core::{FocusDepth, FocusRequest, GraphMode, GraphSnapshot, SearchResult};
use serde::{Deserialize, Serialize};

use crate::services::references;
use crate::state::AppStateHandle;

#[derive(Debug, Deserialize)]
pub(crate) struct SnapshotQuery {
    mode: Option<GraphMode>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchQuery {
    q: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchResponse {
    results: Vec<SearchResult>,
}

pub(crate) async fn snapshot(
    State(state): State<AppStateHandle>,
    Query(query): Query<SnapshotQuery>,
) -> Json<GraphSnapshot> {
    let snapshot = state.graph.read().clone();
    Json(
        query
            .mode
            .map_or(snapshot.clone(), |mode| filter_snapshot(&snapshot, mode)),
    )
}

pub(crate) async fn node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read();
    match graph.nodes.iter().find(|node| node.id == id) {
        Some(node) => (StatusCode::OK, Json(node.clone())).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

pub(crate) async fn node_details(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let indexes = graph_query::build_graph_indexes(&graph);

    let Some(node) = indexes
        .node_by_id
        .get(&id)
        .and_then(|index| graph.nodes.get(*index))
        .cloned()
    else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };

    let mut references = graph_query::graph_reference_records_for_node(&graph, &indexes, &id);

    references.extend(references::resolve_rust_references(&state, &graph, &node).await);
    references.extend(references::resolve_python_references(&state, &graph, &node).await);
    references.extend(references::resolve_typescript_references(&state, &graph, &node).await);
    references.extend(references::resolve_qml_references(&state, &graph, &node).await);

    graph_query::dedupe_references(&mut references);

    let diagnostics = state
        .diagnostics_by_node
        .read()
        .get(&id)
        .cloned()
        .unwrap_or_default();

    match graph_query::node_details_base_with_indexes(
        &graph,
        &indexes,
        &id,
        diagnostics,
        references,
    ) {
        Some(details) => (StatusCode::OK, Json(details)).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

pub(crate) async fn search(
    State(state): State<AppStateHandle>,
    Query(query): Query<SearchQuery>,
) -> Json<SearchResponse> {
    let query = query.q.unwrap_or_default();
    let graph = state.graph.read().clone();

    Json(SearchResponse {
        results: graph_query::search_nodes(&graph, &query, 30),
    })
}

pub(crate) async fn focus(
    State(state): State<AppStateHandle>,
    Json(request): Json<FocusRequest>,
) -> impl IntoResponse {
    let depth = match request.depth {
        FocusDepth::Number(depth) => Some(depth),
        FocusDepth::Full(_) => None,
    };
    let graph = state.graph.read();

    match graph_query::focus_subgraph(&graph, &request.node_id, depth) {
        Some(response) => (StatusCode::OK, Json(response)).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}
