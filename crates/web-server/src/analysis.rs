use anyhow::Result;
use graph_builder::{
    build_fallback_graph, build_language_graph, enrich_api_routes_for_files, enrich_file_symbols,
    enrich_syntax_relationships_for_files, mark_rust_source_reachability,
    push_unique_edge_with_confidence, python, qml, typescript,
};
use graph_core::{
    AnalysisEventType, AnalyzerStatus, AppState, DiagnosticRecord, EdgeConfidence, EdgeType,
    GraphNode, GraphPatch, GraphSnapshot, LanguageId, ServerMessage, SymbolIndex, SymbolKindName,
};
use project_indexer::{index_project, scan_project_languages, ProjectLanguageManifest};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tokio::sync::broadcast;
use tokio::time::{timeout, Duration};
use tracing::{info, warn};
use url::Url;

use crate::python_ty::{enrich_python_semantic_calls_for_files, enrich_python_with_ty};
use crate::typescript_lsp::{
    self, enrich_typescript_semantic_edges_for_files, enrich_typescript_with_lsp,
};
use crate::{AnalyzerState, AppStateHandle};

type NodeLayoutState = (f64, f64, f64, f64, Option<bool>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectedAnalyzers {
    python: bool,
    typescript: bool,
    qml: bool,
}

impl DetectedAnalyzers {
    fn all_enabled() -> Self {
        Self {
            python: true,
            typescript: true,
            qml: true,
        }
    }

    fn from_manifest(manifest: &ProjectLanguageManifest) -> Self {
        Self {
            python: manifest.python_files > 0,
            typescript: manifest.typescript_files > 0 || manifest.javascript_files > 0,
            qml: manifest.qml_files > 0,
        }
    }
}

fn detect_project_analyzers(project_root: &Path) -> DetectedAnalyzers {
    match scan_project_languages(project_root) {
        Ok(manifest) => DetectedAnalyzers::from_manifest(&manifest),
        Err(error) => {
            warn!(
                ?error,
                project_root = %project_root.display(),
                "failed to scan project languages; optional analyzers remain enabled"
            );
            DetectedAnalyzers::all_enabled()
        }
    }
}

async fn enrich_optional_analyzers(
    state: &AppStateHandle,
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    detected: DetectedAnalyzers,
) {
    if detected.python {
        start_python_ty_if_available(state).await;
        let _ = enrich_python_with_ty(snapshot, project_root, &state.python_ty).await;
    }
    if detected.qml {
        sync_qml_lsp_snapshot(state, snapshot, project_root).await;
    }
    if detected.typescript {
        enrich_typescript_lsp_snapshot(state, snapshot, project_root).await;
    }
}

pub(crate) async fn index_and_publish(state: AppStateHandle, project_root: PathBuf) {
    if state.is_indexing.swap(true, Ordering::SeqCst) {
        info!(project_root = %project_root.display(), "indexing already in progress, skipping");
        return;
    }
    info!(project_root = %project_root.display(), "indexing start");
    crate::update_status(&state, |status| {
        status.app_state = AppState::Indexing;
        status.analyzer_status = AnalyzerStatus::Starting;
        status.project_name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string);
        status.project_path = Some(project_root.display().to_string());
        status.message = Some("Indexing workspace".into());
        status.progress = Some(5);
    });

    let detected = detect_project_analyzers(&project_root);
    let index = match index_project(&project_root) {
        Ok(index) => index,
        Err(error) => {
            warn!(
                ?error,
                "cargo project index unavailable; building language graph"
            );
            crate::update_status(&state, |status| {
                status.app_state = AppState::Normal;
                status.analyzer_status = AnalyzerStatus::Fallback;
                status.message = Some(
                    "No Cargo.toml found; rust-analyzer disabled; Rust syntax fallback active"
                        .into(),
                );
                status.progress = Some(80);
            });
            let mut snapshot = build_language_graph(&project_root, state.status.read().clone());
            enrich_optional_analyzers(&state, &mut snapshot, &project_root, detected).await;
            snapshot.status = crate::fallback_status(
                &state,
                "No Cargo.toml found; rust-analyzer disabled; Rust syntax fallback active",
            );
            crate::publish_snapshot(&state, snapshot);
            state.is_indexing.store(false, Ordering::SeqCst);
            return;
        }
    };

    crate::update_status(&state, |status| {
        status.analyzer_status = AnalyzerStatus::Indexing;
        status.message = Some("Building fallback graph".into());
        status.progress = Some(25);
    });

    let fallback_status = state.status.read().clone();
    let mut snapshot = build_fallback_graph(&index, fallback_status);
    crate::publish_snapshot(&state, snapshot.clone());

    crate::update_status(&state, |status| {
        status.message = Some("Starting rust-analyzer".into());
        status.progress = Some(40);
    });

    match state.analyzer.subscribe_notifications().await {
        Ok(rx) => spawn_diagnostics_listener(state.clone(), rx),
        Err(error) => {
            warn!(?error, "rust-analyzer unavailable, using fallback graph");
            publish_analyzer_fallback(
                &state,
                snapshot,
                "rust-analyzer is unavailable. Using syntax graph fallback.",
            );
            info!(
                nodes = state.graph.read().nodes.len(),
                edges = state.graph.read().edges.len(),
                files = state.graph.read().files.len(),
                "indexing finish"
            );
            state.is_indexing.store(false, Ordering::SeqCst);
            return;
        }
    }

    {
        crate::update_status(&state, |status| {
            status.message = Some("Reading document symbols".into());
            status.progress = Some(55);
        });
        for (idx, file) in index.files.iter().enumerate() {
            match timeout(
                Duration::from_secs(3),
                state.analyzer.document_symbols(&file.absolute_path),
            )
            .await
            {
                Ok(Ok(symbols)) => enrich_file_symbols(&mut snapshot, file, &symbols),
                Ok(Err(error)) => {
                    warn!(file = %file.relative_path, ?error, "documentSymbol failed")
                }
                Err(_) => warn!(file = %file.relative_path, "documentSymbol timed out"),
            }
            let progress = 55 + ((idx as f32 / index.files.len().max(1) as f32) * 35.0) as u8;
            crate::update_status(&state, |status| status.progress = Some(progress.min(90)));
        }
        crate::update_status(&state, |status| {
            status.message = Some("Resolving semantic call graph".into());
            status.progress = Some(92);
        });
        enrich_semantic_call_edges(&mut snapshot, &project_root, &state.analyzer).await;
        enrich_optional_analyzers(&state, &mut snapshot, &project_root, detected).await;
        snapshot.status = crate::ready_status(&state, "Ready");
        crate::publish_snapshot(&state, snapshot);
    }

    info!(
        nodes = state.graph.read().nodes.len(),
        edges = state.graph.read().edges.len(),
        files = state.graph.read().files.len(),
        "indexing finish"
    );
    state.is_indexing.store(false, Ordering::SeqCst);
}

pub(crate) async fn index_and_patch(
    state: AppStateHandle,
    project_root: PathBuf,
    changed_files: Vec<String>,
) {
    if state.is_indexing.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::update_status(&state, |status| {
        status.analyzer_status = AnalyzerStatus::Indexing;
        status.message = Some("Updating changed files".into());
        status.progress = Some(20);
    });

    let ts_files = changed_files
        .iter()
        .filter(|file| typescript::is_typescript_path(file))
        .cloned()
        .collect::<Vec<_>>();
    let only_typescript = !ts_files.is_empty()
        && changed_files
            .iter()
            .all(|file| typescript::is_typescript_path(file));
    let python_files = changed_files
        .iter()
        .filter(|file| python::is_python_path(file))
        .cloned()
        .collect::<Vec<_>>();
    let only_python = !python_files.is_empty()
        && changed_files
            .iter()
            .all(|file| python::is_python_path(file));
    let qml_files = changed_files
        .iter()
        .filter(|file| qml::is_qml_path(file))
        .cloned()
        .collect::<Vec<_>>();
    let only_qml = !qml_files.is_empty() && changed_files.iter().all(|file| qml::is_qml_path(file));
    let index = match index_project(&project_root) {
        Ok(index) => Some(index),
        Err(error) => {
            warn!(?error, "cargo project index unavailable during patch");
            None
        }
    };
    let rust_files = index
        .as_ref()
        .map(|index| {
            changed_files
                .iter()
                .filter(|file| file.ends_with(".rs"))
                .filter_map(|file| {
                    index
                        .files
                        .iter()
                        .find(|indexed| indexed.relative_path == *file)
                        .cloned()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let only_rust = !rust_files.is_empty()
        && changed_files
            .iter()
            .all(|file| file.ends_with(".rs") || file.ends_with("Cargo.toml"));

    if only_rust {
        if let Some(index) = index.as_ref() {
            match index_changed_rust_files(
                &state,
                &project_root,
                index,
                rust_files,
                changed_files.clone(),
            )
            .await
            {
                Ok(()) => {
                    state.is_indexing.store(false, Ordering::SeqCst);
                    return;
                }
                Err(error) => warn!(
                    ?error,
                    "incremental file patch failed; falling back to rebuild patch"
                ),
            }
        }
    }
    if only_typescript {
        match index_changed_typescript_files(&state, &project_root, ts_files, changed_files.clone())
            .await
        {
            Ok(()) => {
                state.is_indexing.store(false, Ordering::SeqCst);
                return;
            }
            Err(error) => warn!(
                ?error,
                "incremental TypeScript patch failed; falling back to rebuild patch"
            ),
        }
    }
    if only_python {
        match index_changed_python_files(&state, &project_root, python_files, changed_files.clone())
            .await
        {
            Ok(()) => {
                state.is_indexing.store(false, Ordering::SeqCst);
                return;
            }
            Err(error) => warn!(
                ?error,
                "incremental Python patch failed; falling back to rebuild patch"
            ),
        }
    }
    if only_qml {
        match index_changed_qml_files(&state, &project_root, qml_files, changed_files.clone()).await
        {
            Ok(()) => {
                state.is_indexing.store(false, Ordering::SeqCst);
                return;
            }
            Err(error) => warn!(
                ?error,
                "incremental QML patch failed; falling back to rebuild patch"
            ),
        }
    }

    let detected = detect_project_analyzers(&project_root);
    if let Some(index) = index {
        rebuild_patch_snapshot(state, project_root, index, changed_files, detected).await;
    } else {
        rebuild_language_patch_snapshot(state, project_root, changed_files, detected).await;
    }
}

async fn rebuild_patch_snapshot(
    state: AppStateHandle,
    project_root: PathBuf,
    index: project_indexer::ProjectIndex,
    changed_files: Vec<String>,
    detected: DetectedAnalyzers,
) {
    let old_snapshot = state.graph.read().clone();

    let mut snapshot = build_fallback_graph(&index, state.status.read().clone());
    if state.analyzer.subscribe_notifications().await.is_ok() {
        for file in &index.files {
            match timeout(
                Duration::from_secs(3),
                state.analyzer.document_symbols(&file.absolute_path),
            )
            .await
            {
                Ok(Ok(symbols)) => enrich_file_symbols(&mut snapshot, file, &symbols),
                Ok(Err(error)) => {
                    warn!(file = %file.relative_path, ?error, "documentSymbol failed during patch")
                }
                Err(_) => {
                    warn!(file = %file.relative_path, "documentSymbol timed out during patch")
                }
            }
        }
        enrich_semantic_call_edges(&mut snapshot, &project_root, &state.analyzer).await;
    }
    enrich_optional_analyzers(&state, &mut snapshot, &project_root, detected).await;
    snapshot.status = crate::ready_status(&state, "Ready");
    let diagnostics = state
        .diagnostics_by_file
        .read()
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    apply_diagnostics_to_files(&mut snapshot, &diagnostics);
    let patch = diff_snapshots(&old_snapshot, &snapshot, changed_files, diagnostics);
    *state.graph.write() = snapshot;
    let _ = state.ws_tx.send(ServerMessage::GraphPatch(patch));
    state.is_indexing.store(false, Ordering::SeqCst);
}

async fn rebuild_language_patch_snapshot(
    state: AppStateHandle,
    project_root: PathBuf,
    changed_files: Vec<String>,
    detected: DetectedAnalyzers,
) {
    let old_snapshot = state.graph.read().clone();
    let mut snapshot = build_language_graph(&project_root, state.status.read().clone());
    enrich_optional_analyzers(&state, &mut snapshot, &project_root, detected).await;
    snapshot.status = crate::fallback_status(
        &state,
        "No Cargo.toml found; rust-analyzer disabled; Rust syntax fallback active",
    );
    let diagnostics = state
        .diagnostics_by_file
        .read()
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    apply_diagnostics_to_files(&mut snapshot, &diagnostics);
    let patch = diff_snapshots(&old_snapshot, &snapshot, changed_files, diagnostics);
    *state.graph.write() = snapshot;
    let _ = state.ws_tx.send(ServerMessage::GraphPatch(patch));
    state.is_indexing.store(false, Ordering::SeqCst);
}

async fn index_changed_rust_files(
    state: &AppStateHandle,
    project_root: &Path,
    index: &project_indexer::ProjectIndex,
    files: Vec<project_indexer::IndexedFile>,
    changed_files: Vec<String>,
) -> Result<()> {
    let old_snapshot = state.graph.read().clone();
    let mut snapshot = old_snapshot.clone();
    let changed_set = files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect::<HashSet<_>>();
    let old_positions = old_snapshot
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node_layout_state(node)))
        .collect::<HashMap<_, _>>();

    for file in &files {
        state
            .analyzer
            .sync_changed_file(&file.absolute_path)
            .await?;
    }

    remove_file_symbols_and_edges(&mut snapshot, &changed_set);
    for file in &files {
        let symbols = match timeout(
            Duration::from_secs(3),
            state.analyzer.document_symbols(&file.absolute_path),
        )
        .await
        {
            Ok(Ok(symbols)) => symbols,
            Ok(Err(error)) => {
                warn!(file = %file.relative_path, ?error, "documentSymbol failed for changed file");
                graph_builder::discover_syntax_symbols(file)
            }
            Err(_) => {
                warn!(file = %file.relative_path, "documentSymbol timed out for changed file");
                graph_builder::discover_syntax_symbols(file)
            }
        };
        enrich_file_symbols(&mut snapshot, file, &symbols);
    }
    enrich_syntax_relationships_for_files(&mut snapshot, &files);
    enrich_api_routes_for_files(&mut snapshot, &files);
    enrich_semantic_call_edges_for_files(
        &mut snapshot,
        project_root,
        &state.analyzer,
        &changed_set,
    )
    .await;
    mark_rust_source_reachability(&mut snapshot, index);
    restore_existing_positions(&mut snapshot, &old_positions);
    snapshot.status = crate::ready_status(state, "Ready");
    let diagnostics = state
        .diagnostics_by_file
        .read()
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    apply_diagnostics_to_files(&mut snapshot, &diagnostics);
    let patch = diff_snapshots(&old_snapshot, &snapshot, changed_files, diagnostics);
    *state.graph.write() = snapshot;
    let _ = state.ws_tx.send(ServerMessage::GraphPatch(patch));
    Ok(())
}

async fn index_changed_typescript_files(
    state: &AppStateHandle,
    project_root: &Path,
    files: Vec<String>,
    changed_files: Vec<String>,
) -> Result<()> {
    let old_snapshot = state.graph.read().clone();
    let mut snapshot = old_snapshot.clone();
    let changed_set = files.into_iter().collect::<HashSet<_>>();
    let old_positions = old_snapshot
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node_layout_state(node)))
        .collect::<HashMap<_, _>>();

    remove_file_symbols_and_edges(&mut snapshot, &changed_set);
    if !state.typescript_lsp.is_parser_only() {
        for file in &changed_set {
            let absolute = project_root.join(file);
            if let Err(error) = state.typescript_lsp.sync_changed_file(&absolute).await {
                warn!(?error, file = %file, "typescript didChange failed; keeping parser TypeScript incremental path");
            }
        }
    }
    graph_builder::typescript::enrich_typescript_graph_for_files(
        &mut snapshot,
        project_root,
        &changed_set,
    );
    if !state.typescript_lsp.is_parser_only() {
        for file in &changed_set {
            let absolute = project_root.join(file);
            match timeout(
                Duration::from_secs(3),
                state.typescript_lsp.document_symbols(&absolute),
            )
            .await
            {
                Ok(Ok(symbols)) => {
                    typescript_lsp::enrich_nodes_from_lsp_symbols(&mut snapshot, file, &symbols)
                }
                Ok(Err(error)) => {
                    warn!(?error, file = %file, "typescript documentSymbol failed for changed file")
                }
                Err(_) => {
                    warn!(file = %file, "typescript documentSymbol timed out for changed file")
                }
            }
        }
        enrich_typescript_semantic_edges_for_files(
            &mut snapshot,
            project_root,
            &state.typescript_lsp,
            &changed_set,
        )
        .await;
    }
    restore_existing_positions(&mut snapshot, &old_positions);
    snapshot.status = crate::ready_status(state, "Ready");
    let diagnostics = state
        .diagnostics_by_file
        .read()
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    apply_diagnostics_to_files(&mut snapshot, &diagnostics);
    let patch = diff_snapshots(&old_snapshot, &snapshot, changed_files, diagnostics);
    *state.graph.write() = snapshot;
    let _ = state.ws_tx.send(ServerMessage::GraphPatch(patch));
    Ok(())
}

async fn index_changed_python_files(
    state: &AppStateHandle,
    project_root: &Path,
    files: Vec<String>,
    changed_files: Vec<String>,
) -> Result<()> {
    let old_snapshot = state.graph.read().clone();
    let mut snapshot = old_snapshot.clone();
    let changed_set = files.into_iter().collect::<HashSet<_>>();
    let old_positions = old_snapshot
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node_layout_state(node)))
        .collect::<HashMap<_, _>>();

    remove_file_symbols_and_edges(&mut snapshot, &changed_set);
    if !state.python_ty.is_parser_only() {
        for file in &changed_set {
            let absolute = project_root.join(file);
            if let Err(error) = state.python_ty.sync_changed_file(&absolute).await {
                warn!(?error, file = %file, "ty didChange failed; keeping parser Python incremental path");
            }
        }
    }
    graph_builder::python::enrich_python_graph_for_files(&mut snapshot, project_root, &changed_set);
    enrich_python_semantic_calls_for_files(
        &mut snapshot,
        project_root,
        &state.python_ty,
        &changed_set,
    )
    .await;
    restore_existing_positions(&mut snapshot, &old_positions);
    snapshot.status = crate::ready_status(state, "Ready");
    let diagnostics = state
        .diagnostics_by_file
        .read()
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    apply_diagnostics_to_files(&mut snapshot, &diagnostics);
    let patch = diff_snapshots(&old_snapshot, &snapshot, changed_files, diagnostics);
    *state.graph.write() = snapshot;
    let _ = state.ws_tx.send(ServerMessage::GraphPatch(patch));
    Ok(())
}

async fn index_changed_qml_files(
    state: &AppStateHandle,
    project_root: &Path,
    files: Vec<String>,
    changed_files: Vec<String>,
) -> Result<()> {
    let old_snapshot = state.graph.read().clone();
    let mut snapshot = old_snapshot.clone();
    let changed_set = files.into_iter().collect::<HashSet<_>>();
    let old_positions = old_snapshot
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node_layout_state(node)))
        .collect::<HashMap<_, _>>();

    remove_file_symbols_and_edges(&mut snapshot, &changed_set);
    graph_builder::qml::enrich_qml_graph_for_files(&mut snapshot, project_root, &changed_set);
    if !state.qml_lsp.is_parser_only() {
        for file in &changed_set {
            let absolute = project_root.join(file);
            if let Err(error) = state.qml_lsp.sync_changed_file(&absolute).await {
                warn!(?error, file = %file, "qmlls didChange failed; keeping parser QML incremental path");
            }
        }
    }
    restore_existing_positions(&mut snapshot, &old_positions);
    snapshot.status = crate::ready_status(state, "Ready");
    let diagnostics = state
        .diagnostics_by_file
        .read()
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    apply_diagnostics_to_files(&mut snapshot, &diagnostics);
    let patch = diff_snapshots(&old_snapshot, &snapshot, changed_files, diagnostics);
    *state.graph.write() = snapshot;
    let _ = state.ws_tx.send(ServerMessage::GraphPatch(patch));
    Ok(())
}

fn remove_file_symbols_and_edges(snapshot: &mut GraphSnapshot, changed_files: &HashSet<String>) {
    let removed = snapshot
        .nodes
        .iter()
        .filter(|node| {
            node.file
                .as_ref()
                .is_some_and(|file| changed_files.contains(file))
                && node.node_type != graph_core::NodeType::File
        })
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    snapshot.nodes.retain(|node| !removed.contains(&node.id));
    snapshot
        .edges
        .retain(|edge| !removed.contains(&edge.source) && !removed.contains(&edge.target));
}

fn restore_existing_positions(
    snapshot: &mut GraphSnapshot,
    old_positions: &HashMap<String, NodeLayoutState>,
) {
    for node in &mut snapshot.nodes {
        if let Some((x, y, vx, vy, pinned)) = old_positions.get(&node.id) {
            node.x = *x;
            node.y = *y;
            node.vx = *vx;
            node.vy = *vy;
            node.pinned = *pinned;
        }
    }
}

fn node_layout_state(node: &GraphNode) -> NodeLayoutState {
    (node.x, node.y, node.vx, node.vy, node.pinned)
}

fn diff_snapshots(
    old: &GraphSnapshot,
    new: &GraphSnapshot,
    changed_files: Vec<String>,
    diagnostics: Vec<DiagnosticRecord>,
) -> GraphPatch {
    let old_nodes = old
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let new_nodes = new
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let old_edges = old
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let new_edges = new
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<HashMap<_, _>>();

    GraphPatch {
        added_nodes: new
            .nodes
            .iter()
            .filter(|node| !old_nodes.contains_key(node.id.as_str()))
            .cloned()
            .collect(),
        updated_nodes: new
            .nodes
            .iter()
            .filter(|node| {
                old_nodes.get(node.id.as_str()).is_some_and(|old| {
                    serde_json::to_value(old).ok() != serde_json::to_value(node).ok()
                })
            })
            .cloned()
            .collect(),
        removed_node_ids: old
            .nodes
            .iter()
            .filter(|node| !new_nodes.contains_key(node.id.as_str()))
            .map(|node| node.id.clone())
            .collect(),
        added_edges: new
            .edges
            .iter()
            .filter(|edge| !old_edges.contains_key(edge.id.as_str()))
            .cloned()
            .collect(),
        updated_edges: new
            .edges
            .iter()
            .filter(|edge| {
                old_edges.get(edge.id.as_str()).is_some_and(|old| {
                    serde_json::to_value(old).ok() != serde_json::to_value(edge).ok()
                })
            })
            .cloned()
            .collect(),
        removed_edge_ids: old
            .edges
            .iter()
            .filter(|edge| !new_edges.contains_key(edge.id.as_str()))
            .map(|edge| edge.id.clone())
            .collect(),
        diagnostics,
        changed_files,
    }
}

fn apply_diagnostics_to_files(snapshot: &mut GraphSnapshot, diagnostics: &[DiagnosticRecord]) {
    let mut by_file: HashMap<&str, u32> = HashMap::new();
    for diagnostic in diagnostics {
        *by_file.entry(diagnostic.file.as_str()).or_default() += 1;
    }
    for file in &mut snapshot.files {
        file.diagnostics_count = by_file.get(file.path.as_str()).copied().unwrap_or_default();
    }
}

fn publish_analyzer_fallback(
    state: &AppStateHandle,
    mut snapshot: GraphSnapshot,
    message: &'static str,
) {
    snapshot.status = crate::fallback_status(state, message);
    snapshot.events.push(crate::analysis_event(
        AnalysisEventType::Warning,
        message,
        None,
    ));
    crate::publish_snapshot(state, snapshot);
}

fn spawn_diagnostics_listener(
    state: AppStateHandle,
    mut rx: broadcast::Receiver<ra_client::LspNotification>,
) {
    tokio::spawn(async move {
        while let Ok(notification) = rx.recv().await {
            if notification.method != "textDocument/publishDiagnostics" {
                continue;
            }
            let Ok(params) = serde_json::from_value::<ra_client::LspPublishDiagnosticsParams>(
                notification.params,
            ) else {
                continue;
            };
            crate::apply_lsp_diagnostics(&state, Some(LanguageId::Rust), None, params);
        }
    });
}

async fn start_python_ty_if_available(state: &AppStateHandle) -> bool {
    if state.python_ty.is_parser_only() {
        crate::update_status(state, |_| {});
        return false;
    }
    match state.python_ty.subscribe_notifications().await {
        Ok(rx) => {
            spawn_python_diagnostics_listener(state.clone(), rx);
            crate::update_status(state, |_| {});
            true
        }
        Err(error) => {
            if state.python_ty.should_log_start_failure() {
                warn!(
                    ?error,
                    "ty unavailable; Python parser fallback remains active"
                );
            }
            let status_record = state.python_ty.status_record();
            crate::update_status(state, |status| {
                if status_record.mode == "ty" {
                    status.analyzer_status = AnalyzerStatus::Error;
                    status.message = Some(format!("Python analyzer ty unavailable: {error}"));
                }
            });
            false
        }
    }
}

async fn start_typescript_lsp_if_available(state: &AppStateHandle) -> bool {
    if state.typescript_lsp.is_parser_only() {
        crate::update_status(state, |_| {});
        return false;
    }
    match state.typescript_lsp.subscribe_notifications().await {
        Ok(rx) => {
            spawn_typescript_diagnostics_listener(state.clone(), rx);
            crate::update_status(state, |_| {});
            true
        }
        Err(error) => {
            if state.typescript_lsp.should_log_start_failure() {
                warn!(
                    ?error,
                    "typescript-language-server unavailable; TypeScript parser fallback remains active"
                );
            }
            let status_record = state.typescript_lsp.status_record();
            crate::update_status(state, |status| {
                if status_record.mode == "typescript-language-server" {
                    status.analyzer_status = AnalyzerStatus::Error;
                    status.message =
                        Some(format!("TypeScript language server unavailable: {error}"));
                }
            });
            false
        }
    }
}

async fn start_qml_lsp_if_available(state: &AppStateHandle) -> bool {
    if state.qml_lsp.is_parser_only() {
        crate::update_status(state, |_| {});
        return false;
    }
    match state.qml_lsp.subscribe_notifications().await {
        Ok(rx) => {
            spawn_qml_diagnostics_listener(state.clone(), rx);
            crate::update_status(state, |_| {});
            true
        }
        Err(error) => {
            if state.qml_lsp.should_log_start_failure() {
                warn!(
                    ?error,
                    "qmlls unavailable; QML parser fallback remains active"
                );
            }
            let status_record = state.qml_lsp.status_record();
            crate::update_status(state, |status| {
                if status_record.mode == "qmlls" {
                    status.analyzer_status = AnalyzerStatus::Error;
                    status.message = Some(format!("QML language server unavailable: {error}"));
                }
            });
            false
        }
    }
}

async fn sync_qml_lsp_snapshot(
    state: &AppStateHandle,
    snapshot: &GraphSnapshot,
    project_root: &Path,
) {
    if !start_qml_lsp_if_available(state).await {
        return;
    }
    let files = snapshot
        .files
        .iter()
        .filter(|file| qml::is_qml_path(&file.path))
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    for file in files {
        let absolute = project_root.join(&file);
        if let Err(error) = state.qml_lsp.open_document(&absolute).await {
            warn!(?error, file = %file, "qmlls didOpen failed");
        }
    }
}

async fn enrich_typescript_lsp_snapshot(
    state: &AppStateHandle,
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
) {
    if !start_typescript_lsp_if_available(state).await {
        return;
    }
    if let Err(error) =
        enrich_typescript_with_lsp(snapshot, project_root, &state.typescript_lsp).await
    {
        warn!(
            ?error,
            "typescript-language-server symbol enrichment failed"
        );
    }
    let changed_files = snapshot
        .files
        .iter()
        .filter(|file| typescript::is_typescript_path(&file.path))
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    enrich_typescript_semantic_edges_for_files(
        snapshot,
        project_root,
        &state.typescript_lsp,
        &changed_files,
    )
    .await;
}

fn spawn_python_diagnostics_listener(
    state: AppStateHandle,
    mut rx: broadcast::Receiver<ra_client::LspNotification>,
) {
    tokio::spawn(async move {
        while let Ok(notification) = rx.recv().await {
            if notification.method != "textDocument/publishDiagnostics" {
                continue;
            }
            let Ok(params) = serde_json::from_value::<ra_client::LspPublishDiagnosticsParams>(
                notification.params,
            ) else {
                continue;
            };
            crate::apply_lsp_diagnostics(&state, Some(LanguageId::Python), None, params);
        }
    });
}

fn spawn_typescript_diagnostics_listener(
    state: AppStateHandle,
    mut rx: broadcast::Receiver<ra_client::LspNotification>,
) {
    tokio::spawn(async move {
        while let Ok(notification) = rx.recv().await {
            if notification.method != "textDocument/publishDiagnostics" {
                continue;
            }
            let Ok(params) = serde_json::from_value::<ra_client::LspPublishDiagnosticsParams>(
                notification.params,
            ) else {
                continue;
            };
            crate::apply_lsp_diagnostics(&state, None, Some("typescript-language-server"), params);
        }
    });
}

fn spawn_qml_diagnostics_listener(
    state: AppStateHandle,
    mut rx: broadcast::Receiver<ra_client::LspNotification>,
) {
    tokio::spawn(async move {
        while let Ok(notification) = rx.recv().await {
            if notification.method != "textDocument/publishDiagnostics" {
                continue;
            }
            let Ok(params) = serde_json::from_value::<ra_client::LspPublishDiagnosticsParams>(
                notification.params,
            ) else {
                continue;
            };
            crate::apply_lsp_diagnostics(&state, Some(LanguageId::Qml), Some("qmlls"), params);
        }
    });
}

async fn enrich_semantic_call_edges(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    analyzer: &AnalyzerState,
) {
    let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);
    if symbol_index.symbols.is_empty() {
        return;
    }
    let callable_symbols = symbol_index
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.language == LanguageId::Rust
                && matches!(
                    symbol.kind,
                    SymbolKindName::Function | SymbolKindName::Method
                )
        })
        .map(|symbol| {
            (
                symbol.node_id.clone(),
                project_root.join(&symbol.file),
                symbol.selection_range.start,
            )
        })
        .collect::<Vec<_>>();

    for (source_id, file, position) in callable_symbols {
        let items = match timeout(
            Duration::from_secs(2),
            analyzer.prepare_call_hierarchy(&file, position.line, position.character),
        )
        .await
        {
            Ok(Ok(items)) => items,
            Ok(Err(error)) => {
                warn!(?error, source = %source_id, "prepareCallHierarchy failed");
                continue;
            }
            Err(_) => {
                warn!(source = %source_id, "prepareCallHierarchy timed out");
                continue;
            }
        };
        for item in items {
            let outgoing =
                match timeout(Duration::from_secs(2), analyzer.outgoing_calls(&item)).await {
                    Ok(Ok(outgoing)) => outgoing,
                    Ok(Err(error)) => {
                        warn!(?error, source = %source_id, "outgoingCalls failed");
                        continue;
                    }
                    Err(_) => {
                        warn!(source = %source_id, "outgoingCalls timed out");
                        continue;
                    }
                };
            for call in outgoing {
                let Some(target_path) = Url::parse(call.to.uri.as_str())
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok())
                else {
                    continue;
                };
                insert_semantic_call_edge(
                    snapshot,
                    &symbol_index,
                    &source_id,
                    &target_path,
                    call.to.selection_range.start.line,
                    call.to.selection_range.start.character,
                );
            }
        }
    }
}

async fn enrich_semantic_call_edges_for_files(
    snapshot: &mut GraphSnapshot,
    project_root: &Path,
    analyzer: &AnalyzerState,
    changed_files: &HashSet<String>,
) {
    let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);
    if symbol_index.symbols.is_empty() {
        return;
    }
    let callable_symbols = symbol_index
        .symbols
        .iter()
        .filter(|symbol| {
            changed_files.contains(&symbol.file)
                && symbol.language == LanguageId::Rust
                && matches!(
                    symbol.kind,
                    SymbolKindName::Function | SymbolKindName::Method
                )
        })
        .map(|symbol| {
            (
                symbol.node_id.clone(),
                project_root.join(&symbol.file),
                symbol.selection_range.start,
            )
        })
        .collect::<Vec<_>>();

    for (source_id, file, position) in callable_symbols {
        let items = match timeout(
            Duration::from_secs(2),
            analyzer.prepare_call_hierarchy(&file, position.line, position.character),
        )
        .await
        {
            Ok(Ok(items)) => items,
            _ => continue,
        };
        for item in items {
            let outgoing =
                match timeout(Duration::from_secs(2), analyzer.outgoing_calls(&item)).await {
                    Ok(Ok(outgoing)) => outgoing,
                    _ => continue,
                };
            for call in outgoing {
                let Some(target_path) = Url::parse(call.to.uri.as_str())
                    .ok()
                    .and_then(|uri| uri.to_file_path().ok())
                else {
                    continue;
                };
                insert_semantic_call_edge(
                    snapshot,
                    &symbol_index,
                    &source_id,
                    &target_path,
                    call.to.selection_range.start.line,
                    call.to.selection_range.start.character,
                );
            }
        }
    }
}

fn insert_semantic_call_edge(
    snapshot: &mut GraphSnapshot,
    symbol_index: &SymbolIndex,
    source_id: &str,
    target_path: &Path,
    line: u32,
    character: u32,
) -> bool {
    let Some(target) = symbol_index.find_by_uri_path_position(target_path, line, character) else {
        return false;
    };
    push_unique_edge_with_confidence(
        &mut snapshot.edges,
        &HashSet::new(),
        EdgeType::Calls,
        source_id,
        &target.node_id,
        EdgeConfidence::Semantic,
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{AppStatus, LspPosition, LspRange, Visibility};

    fn test_node(label: &str, file: Option<&str>, module: Option<&str>) -> GraphNode {
        let range = LspRange {
            start: LspPosition {
                line: 0,
                character: 0,
            },
            end: LspPosition {
                line: 0,
                character: label.len() as u32,
            },
        };
        GraphNode {
            id: format!("fn:{}@1", label),
            language: Some("rust".into()),
            node_type: graph_core::NodeType::Function,
            label: label.into(),
            file: file.map(str::to_string),
            module: module.map(str::to_string),
            crate_name: Some("demo".into()),
            line: Some(1),
            visibility: Some(Visibility::Pub),
            is_async: None,
            is_unsafe: None,
            is_generic: None,
            signature: None,
            description: None,
            pinned: None,
            bookmarked: None,
            connections: None,
            range: Some(range),
            selection_range: Some(range),
            reachability: None,
            reachable_from: None,
            detached_reason: None,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
        }
    }

    fn test_edge(
        edge_type: EdgeType,
        source: impl Into<String>,
        target: impl Into<String>,
        confidence: EdgeConfidence,
    ) -> graph_core::GraphEdge {
        let source = source.into();
        let target = target.into();
        graph_core::GraphEdge {
            id: graph_core::edge_id(edge_type, &source, &target),
            source,
            target,
            edge_type,
            confidence,
            label: None,
            description: None,
            data_flow_kind: None,
            evidence: None,
        }
    }

    fn language_manifest(
        python_files: usize,
        typescript_files: usize,
        javascript_files: usize,
        qml_files: usize,
    ) -> ProjectLanguageManifest {
        let total_supported_files = python_files + typescript_files + javascript_files + qml_files;
        ProjectLanguageManifest {
            root: PathBuf::from("/tmp/project"),
            has_cargo: false,
            has_package_json: false,
            has_pyproject: false,
            has_qml: qml_files > 0,
            rust_files: 0,
            python_files,
            typescript_files,
            javascript_files,
            qml_files,
            total_supported_files,
        }
    }

    #[test]
    fn detected_analyzers_disable_optional_languages_when_absent() {
        let detected = DetectedAnalyzers::from_manifest(&language_manifest(0, 0, 0, 0));

        assert_eq!(
            detected,
            DetectedAnalyzers {
                python: false,
                typescript: false,
                qml: false,
            }
        );
    }

    #[test]
    fn detected_analyzers_enable_python_when_python_files_exist() {
        let detected = DetectedAnalyzers::from_manifest(&language_manifest(2, 0, 0, 0));

        assert_eq!(
            detected,
            DetectedAnalyzers {
                python: true,
                typescript: false,
                qml: false,
            }
        );
    }

    #[test]
    fn detected_analyzers_enable_typescript_for_typescript_or_javascript_files() {
        let typescript_detected = DetectedAnalyzers::from_manifest(&language_manifest(0, 1, 0, 0));
        let javascript_detected = DetectedAnalyzers::from_manifest(&language_manifest(0, 0, 1, 0));

        assert!(typescript_detected.typescript);
        assert!(javascript_detected.typescript);
        assert!(!typescript_detected.python);
        assert!(!javascript_detected.qml);
    }

    #[test]
    fn detected_analyzers_enable_qml_when_qml_files_exist() {
        let detected = DetectedAnalyzers::from_manifest(&language_manifest(0, 0, 0, 3));

        assert_eq!(
            detected,
            DetectedAnalyzers {
                python: false,
                typescript: false,
                qml: true,
            }
        );
    }

    #[test]
    fn semantic_call_edge_insertion_resolves_target_from_symbol_index() {
        let source = test_node("main", Some("src/main.rs"), Some("app"));
        let mut target = test_node("helper", Some("src/main.rs"), Some("app"));
        let target_range = LspRange {
            start: LspPosition {
                line: 4,
                character: 0,
            },
            end: LspPosition {
                line: 4,
                character: 6,
            },
        };
        target.line = Some(5);
        target.range = Some(target_range);
        target.selection_range = Some(target_range);
        let mut snapshot = GraphSnapshot {
            nodes: vec![source.clone(), target.clone()],
            edges: vec![test_edge(
                EdgeType::Calls,
                source.id.clone(),
                target.id.clone(),
                EdgeConfidence::SyntaxFallback,
            )],
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        let symbol_index = SymbolIndex::from_nodes(&snapshot.nodes);

        assert!(insert_semantic_call_edge(
            &mut snapshot,
            &symbol_index,
            &source.id,
            Path::new("/tmp/project/src/main.rs"),
            4,
            0,
        ));
        let edge = snapshot
            .edges
            .iter()
            .find(|edge| edge.source == source.id && edge.target == target.id)
            .unwrap();
        assert_eq!(edge.confidence, EdgeConfidence::Semantic);
    }

    #[test]
    fn changed_file_removal_keeps_unrelated_nodes() {
        let mut snapshot = GraphSnapshot {
            nodes: vec![
                test_node("changed", Some("src/changed.rs"), Some("app")),
                test_node("other", Some("src/other.rs"), Some("app")),
            ],
            edges: vec![test_edge(
                EdgeType::Calls,
                "fn:changed@1",
                "fn:other@1",
                EdgeConfidence::SyntaxFallback,
            )],
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        remove_file_symbols_and_edges(&mut snapshot, &HashSet::from(["src/changed.rs".into()]));
        assert!(snapshot.nodes.iter().any(|node| node.id == "fn:other@1"));
        assert!(!snapshot.nodes.iter().any(|node| node.id == "fn:changed@1"));
        assert!(snapshot.edges.is_empty());
    }

    #[test]
    fn unchanged_node_positions_are_preserved() {
        let mut node = test_node("main", Some("src/main.rs"), Some("app"));
        node.x = 42.0;
        node.y = -7.0;
        node.vx = 1.0;
        node.vy = 2.0;
        let positions = HashMap::from([(node.id.clone(), node_layout_state(&node))]);
        let mut updated = test_node("main", Some("src/main.rs"), Some("app"));
        let mut snapshot = GraphSnapshot {
            nodes: vec![updated.clone()],
            edges: Vec::new(),
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        restore_existing_positions(&mut snapshot, &positions);
        updated = snapshot.nodes.remove(0);
        assert_eq!(
            (updated.x, updated.y, updated.vx, updated.vy),
            (42.0, -7.0, 1.0, 2.0)
        );
    }

    #[test]
    fn graph_patch_for_one_file_is_smaller_than_snapshot() {
        let old = GraphSnapshot {
            nodes: vec![
                test_node("main", Some("src/main.rs"), Some("app")),
                test_node("other", Some("src/other.rs"), Some("app")),
            ],
            edges: Vec::new(),
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        let mut new = old.clone();
        new.nodes[0].signature = Some("fn main() {}".into());
        let patch = diff_snapshots(&old, &new, vec!["src/main.rs".into()], Vec::new());
        assert_eq!(patch.updated_nodes.len(), 1);
        assert!(patch.updated_nodes.len() < new.nodes.len());
        assert_eq!(patch.changed_files, vec!["src/main.rs"]);
    }
}
