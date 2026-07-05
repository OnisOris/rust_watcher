use axum::extract::State;
use axum::Json;
use graph_core::DiagnosticRecord;
use serde::Serialize;
use std::collections::HashMap;

use crate::state::AppStateHandle;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticsResponse {
    diagnostics_by_file: HashMap<String, Vec<DiagnosticRecord>>,
    diagnostics_by_node: HashMap<String, Vec<DiagnosticRecord>>,
    all_diagnostics: Vec<DiagnosticRecord>,
}

pub(crate) async fn diagnostics(State(state): State<AppStateHandle>) -> Json<DiagnosticsResponse> {
    let diagnostics_by_file = state.diagnostics_by_file.read().clone();
    let diagnostics_by_node = state.diagnostics_by_node.read().clone();
    let all_diagnostics = diagnostics_by_file
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    Json(DiagnosticsResponse {
        diagnostics_by_file,
        diagnostics_by_node,
        all_diagnostics,
    })
}
