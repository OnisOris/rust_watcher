use graph_core::{
    DiagnosticRecord, DiagnosticSeverity, GraphPatch, LanguageId, ServerMessage, SymbolIndex,
};
use std::collections::HashMap;
use url::Url;

use crate::state::AppStateHandle;
use crate::typescript_lsp::language_for_path;

pub(crate) fn apply_lsp_diagnostics(
    state: &AppStateHandle,
    language: Option<LanguageId>,
    source_override: Option<&str>,
    params: ra_client::LspPublishDiagnosticsParams,
) {
    let Some(path) = Url::parse(params.uri.as_str())
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
    else {
        return;
    };
    let root = state.project_root.read().clone();
    let file = project_indexer::relative_to(&root, &path);
    let graph = state.graph.read().clone();
    let symbol_index = SymbolIndex::from_nodes(&graph.nodes);
    let language = language.unwrap_or_else(|| language_for_path(&file));
    let diagnostics = params
        .diagnostics
        .into_iter()
        .enumerate()
        .map(|(idx, diagnostic)| {
            diagnostic_from_lsp_with_language(
                language.clone(),
                &file,
                idx,
                diagnostic,
                &symbol_index,
                source_override,
            )
        })
        .collect::<Vec<_>>();

    state
        .diagnostics_by_file
        .write()
        .insert(file.clone(), diagnostics.clone());
    rebuild_diagnostics_by_node(state);
    update_project_file_diagnostics(state, &file, diagnostics.len() as u32);

    let _ = state.ws_tx.send(ServerMessage::GraphPatch(GraphPatch {
        diagnostics,
        changed_files: vec![file],
        ..GraphPatch::default()
    }));
}

#[cfg(test)]
pub(crate) fn diagnostic_from_lsp(
    file: &str,
    index: usize,
    diagnostic: ra_client::LspDiagnostic,
    symbol_index: &SymbolIndex,
) -> DiagnosticRecord {
    diagnostic_from_lsp_with_language(
        LanguageId::Rust,
        file,
        index,
        diagnostic,
        symbol_index,
        None,
    )
}

pub(crate) fn diagnostic_from_lsp_with_language(
    language: LanguageId,
    file: &str,
    index: usize,
    diagnostic: ra_client::LspDiagnostic,
    symbol_index: &SymbolIndex,
    source_override: Option<&str>,
) -> DiagnosticRecord {
    let range = graph_core::TextRange {
        start: graph_core::TextPosition {
            line: diagnostic.range.start.line,
            character: diagnostic.range.start.character,
        },
        end: graph_core::TextPosition {
            line: diagnostic.range.end.line,
            character: diagnostic.range.end.character,
        },
    };
    let related_node_ids = related_nodes_for_range(symbol_index, file, range);
    let code = diagnostic.code.map(|code| match code {
        ra_client::LspNumberOrString::Number(value) => value.to_string(),
        ra_client::LspNumberOrString::String(value) => value,
    });
    DiagnosticRecord {
        id: format!(
            "diagnostic:{file}:{}:{}:{index}",
            range.start.line, range.start.character
        ),
        language,
        file: file.to_string(),
        range: Some(range),
        severity: diagnostic_severity(diagnostic.severity),
        source: source_override.map(str::to_string).or(diagnostic.source),
        message: diagnostic.message,
        code,
        related_node_ids,
    }
}

pub(crate) fn diagnostic_severity(
    severity: Option<ra_client::LspDiagnosticSeverity>,
) -> DiagnosticSeverity {
    match severity {
        Some(ra_client::LspDiagnosticSeverity::ERROR) => DiagnosticSeverity::Error,
        Some(ra_client::LspDiagnosticSeverity::WARNING) => DiagnosticSeverity::Warning,
        Some(ra_client::LspDiagnosticSeverity::INFORMATION) => DiagnosticSeverity::Information,
        Some(ra_client::LspDiagnosticSeverity::HINT) => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Information,
    }
}

pub(crate) fn related_nodes_for_range(
    symbol_index: &SymbolIndex,
    file: &str,
    range: graph_core::TextRange,
) -> Vec<String> {
    symbol_index
        .find_by_file(file)
        .into_iter()
        .filter(|symbol| ranges_overlap(symbol.range, range))
        .map(|symbol| symbol.node_id.clone())
        .collect()
}

pub(crate) fn ranges_overlap(left: graph_core::TextRange, right: graph_core::TextRange) -> bool {
    position_le(left.start, right.end) && position_le(right.start, left.end)
}

pub(crate) fn position_le(left: graph_core::TextPosition, right: graph_core::TextPosition) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
}

pub(crate) fn rebuild_diagnostics_by_node(state: &AppStateHandle) {
    let mut by_node: HashMap<String, Vec<DiagnosticRecord>> = HashMap::new();
    for diagnostic in state.diagnostics_by_file.read().values().flatten() {
        for node_id in &diagnostic.related_node_ids {
            by_node
                .entry(node_id.clone())
                .or_default()
                .push(diagnostic.clone());
        }
    }
    *state.diagnostics_by_node.write() = by_node;
}

pub(crate) fn update_project_file_diagnostics(state: &AppStateHandle, file: &str, count: u32) {
    let mut graph = state.graph.write();
    if let Some(project_file) = graph
        .files
        .iter_mut()
        .find(|project_file| project_file.path == file)
    {
        project_file.diagnostics_count = count;
    }
}
