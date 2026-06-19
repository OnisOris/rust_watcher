use graph_core::{
    DiagnosticRecord, DiscoveredSymbol, GraphEdge, GraphSnapshot, LanguageAnalyzer, LanguageId,
    SourceFile, SymbolRecord,
};
use project_indexer::{relative_to, IndexedFile};
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::{
    build_project_files_from_snapshot, collect_language_files, dedupe_graph,
    discover_syntax_symbols_from_source, file_id, push_symbol, symbol_record_from_discovered,
    update_connections,
};

pub struct RustLanguageAdapter;

impl RustLanguageAdapter {
    pub fn enrich_file_symbols(
        &self,
        snapshot: &mut GraphSnapshot,
        file: &IndexedFile,
        symbols: &[DiscoveredSymbol],
    ) {
        let file_node_id = file_id(&file.relative_path);
        let mut new_nodes = Vec::new();
        let mut new_edges = Vec::new();
        for symbol in symbols {
            push_symbol(
                &mut new_nodes,
                &mut new_edges,
                &file_node_id,
                file,
                symbol,
                0,
            );
        }
        snapshot.nodes.extend(new_nodes);
        snapshot.edges.extend(new_edges);
        dedupe_graph(snapshot);
        update_connections(&mut snapshot.nodes, &snapshot.edges);
        snapshot.files = build_project_files_from_snapshot(&snapshot.nodes, &snapshot.edges);
    }
}

impl LanguageAnalyzer for RustLanguageAdapter {
    fn language_id(&self) -> LanguageId {
        LanguageId::Rust
    }

    fn supported_extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn discover_files<'a>(
        &'a self,
        root: &'a Path,
    ) -> Pin<Box<dyn Future<Output = Vec<SourceFile>> + Send + 'a>> {
        Box::pin(async move {
            let mut paths = Vec::new();
            collect_language_files(root, self.supported_extensions(), &mut paths);
            paths
                .into_iter()
                .map(|path| SourceFile {
                    language: LanguageId::Rust,
                    absolute_path: path.display().to_string(),
                    relative_path: relative_to(root, &path),
                    text: fs::read_to_string(&path).ok(),
                })
                .collect()
        })
    }

    fn symbols<'a>(
        &'a self,
        file: &'a SourceFile,
    ) -> Pin<Box<dyn Future<Output = Vec<SymbolRecord>> + Send + 'a>> {
        Box::pin(async move {
            let Some(source) = file.text.as_deref() else {
                return Vec::new();
            };
            discover_syntax_symbols_from_source(source)
                .into_iter()
                .filter_map(|symbol| {
                    symbol_record_from_discovered(LanguageId::Rust, &file.relative_path, symbol)
                })
                .collect()
        })
    }

    fn edges<'a>(
        &'a self,
        _symbols: &'a [SymbolRecord],
    ) -> Pin<Box<dyn Future<Output = Vec<GraphEdge>> + Send + 'a>> {
        Box::pin(async { Vec::new() })
    }

    fn diagnostics<'a>(
        &'a self,
        _file: &'a SourceFile,
    ) -> Pin<Box<dyn Future<Output = Vec<DiagnosticRecord>> + Send + 'a>> {
        Box::pin(async { Vec::new() })
    }
}
