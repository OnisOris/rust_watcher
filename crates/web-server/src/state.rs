use anyhow::Result;
use graph_core::{AppStatus, DiagnosticRecord, GraphSnapshot, ServerMessage};
use graph_query::{GraphIndexes, SearchIndex};
use parking_lot::RwLock;
use ra_client::{LspRuntime, LspRuntimeConfig, LspRuntimeMode};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::analyzer_paths::resolve_rust_analyzer;
use crate::python_ty::PythonTyState;
use crate::qml_lsp::QmlLspState;
use crate::typescript_lsp::TypeScriptLspState;

#[derive(Clone)]
pub(crate) struct AppStateHandle {
    pub(crate) project_root: Arc<RwLock<PathBuf>>,
    pub(crate) graph: Arc<RwLock<GraphSnapshot>>,
    pub(crate) graph_indexes: Arc<RwLock<GraphIndexes>>,
    pub(crate) search_index: Arc<RwLock<SearchIndex>>,
    pub(crate) status: Arc<RwLock<AppStatus>>,
    pub(crate) ws_tx: broadcast::Sender<ServerMessage>,
    pub(crate) analyzer: Arc<AnalyzerState>,
    pub(crate) python_ty: Arc<PythonTyState>,
    pub(crate) typescript_lsp: Arc<TypeScriptLspState>,
    pub(crate) qml_lsp: Arc<QmlLspState>,
    pub(crate) diagnostics_by_file: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    pub(crate) diagnostics_by_node: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    pub(crate) watcher: Arc<RwLock<Option<notify::RecommendedWatcher>>>,
    pub(crate) is_indexing: Arc<AtomicBool>,
    pub(crate) pending_changed_files: Arc<RwLock<HashSet<String>>>,
    pub(crate) pending_changed_files_root: Arc<RwLock<Option<PathBuf>>>,
    pub(crate) watcher_debounce_running: Arc<AtomicBool>,
    pub(crate) watcher_event_generation: Arc<AtomicU64>,
    pub(crate) enable_editor_open: bool,
}

impl AppStateHandle {
    pub(crate) fn replace_graph_snapshot(&self, snapshot: GraphSnapshot) {
        let indexes = graph_query::build_graph_indexes(&snapshot);
        let search_index = graph_query::build_search_index(&snapshot);
        let mut graph = self.graph.write();
        let mut cached_indexes = self.graph_indexes.write();
        let mut cached_search_index = self.search_index.write();
        *graph = snapshot;
        *cached_indexes = indexes;
        *cached_search_index = search_index;
    }
}

pub(crate) struct AnalyzerState {
    pub(crate) runtime: LspRuntime,
}

pub(crate) fn rust_analyzer_state(binary: PathBuf, root: PathBuf) -> AnalyzerState {
    AnalyzerState {
        runtime: LspRuntime::new(LspRuntimeConfig {
            analyzer_id: "rust-analyzer",
            process_name: "rust-analyzer",
            default_language_id: "rust",
            binary,
            args: Vec::new(),
            mode: LspRuntimeMode::Required,
            fallback_message: "rust-analyzer unavailable.",
            resolver: resolve_rust_analyzer,
            root,
        }),
    }
}

#[allow(dead_code)]
impl AnalyzerState {
    pub(crate) async fn set_root(&self, root: PathBuf) {
        self.runtime.set_root(root).await;
    }

    pub(crate) async fn ensure_document_open(&self, file: &Path) -> Result<()> {
        self.runtime.open_document(file, Some("rust")).await
    }

    pub(crate) async fn sync_changed_file(&self, file: &Path) -> Result<i32> {
        self.runtime.sync_changed_file(file, Some("rust")).await
    }

    pub(crate) async fn document_symbols(
        &self,
        file: &Path,
    ) -> Result<Vec<graph_core::DiscoveredSymbol>> {
        self.runtime.document_symbols(file, Some("rust")).await
    }

    pub(crate) async fn prepare_call_hierarchy(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<ra_client::LspCallHierarchyItem>> {
        self.runtime
            .prepare_call_hierarchy(file, line, character, Some("rust"))
            .await
    }

    pub(crate) async fn outgoing_calls(
        &self,
        item: &ra_client::LspCallHierarchyItem,
    ) -> Result<Vec<ra_client::LspCallHierarchyOutgoingCall>> {
        self.runtime.outgoing_calls(item).await
    }

    pub(crate) async fn incoming_calls(
        &self,
        item: &ra_client::LspCallHierarchyItem,
    ) -> Result<Vec<ra_client::LspCallHierarchyIncomingCall>> {
        self.runtime.incoming_calls(item).await
    }

    pub(crate) async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<ra_client::LspLocation>> {
        self.runtime
            .references(file, line, character, Some("rust"))
            .await
    }

    pub(crate) async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .definition(file, line, character, Some("rust"))
            .await
    }

    pub(crate) async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .type_definition(file, line, character, Some("rust"))
            .await
    }

    pub(crate) async fn subscribe_notifications(
        &self,
    ) -> Result<broadcast::Receiver<ra_client::LspNotification>> {
        self.runtime.subscribe_notifications().await
    }
}
