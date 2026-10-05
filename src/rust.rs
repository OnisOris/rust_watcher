use crate::lsp::{path_uri, uri_path, LspClient};
use crate::model::{
    stable_symbol_id, CallNode, CallTarget, CallTargets, CallTree, Diagnostic, Explanation,
    Location, Position, Range, Severity, Symbol, SymbolSearchResult,
};
use crate::project::Project;
use anyhow::{Context, Result};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, DocumentSymbol,
    DocumentSymbolResponse, GotoDefinitionResponse, Hover, HoverContents, Location as LspLocation,
    MarkedString, SymbolInformation,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

const CALL_NODE_BUDGET: usize = 100;
const CALL_CHILD_LIMIT: usize = 20;
const SYMBOL_SEARCH_LIMIT: usize = 50;
const AMBIGUITY_LIMIT: usize = 20;

#[derive(Clone, Copy)]
enum LookupPurpose {
    List,
    Resolve,
}

#[derive(Debug)]
pub enum ResolveError {
    NotFound {
        query: String,
    },
    Ambiguous {
        query: String,
        candidates: Vec<Symbol>,
        truncated: bool,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { query } => write!(formatter, "symbol not found: {query}"),
            Self::Ambiguous {
                query,
                candidates,
                truncated,
            } => {
                writeln!(formatter, "Multiple symbols matched \"{query}\":\n")?;
                for symbol in candidates {
                    writeln!(
                        formatter,
                        "  {:<24} {}:{}",
                        qualified_name(symbol),
                        symbol.file.display(),
                        symbol.selection_range.start.line + 1
                    )?;
                }
                if *truncated {
                    writeln!(formatter, "  … more candidates omitted")?;
                }
                write!(formatter, "\nUse a more specific symbol name.")
            }
        }
    }
}

impl std::error::Error for ResolveError {}

pub struct RustAnalyzer {
    pub project: Project,
    client: LspClient,
    opened: HashSet<PathBuf>,
}

struct TraversalState {
    remaining: usize,
    truncated: bool,
}

struct NormalizedCall {
    key: String,
    name: String,
    item: CallHierarchyItem,
    location: Location,
}

impl RustAnalyzer {
    pub async fn start(project: Project, binary: &Path) -> Result<Self> {
        let client = LspClient::start(binary, &project.workspace_root).await?;
        Ok(Self {
            project,
            client,
            opened: HashSet::new(),
        })
    }

    pub async fn wait_ready(&self) -> Result<()> {
        self.client.wait_ready(Duration::from_secs(30)).await
    }

    pub fn is_ready(&self) -> bool {
        self.client.is_ready()
    }

    pub fn error(&self) -> Option<String> {
        self.client.error()
    }

    async fn open(&mut self, file: &Path) -> Result<bool> {
        let file = if file.is_absolute() {
            file.to_path_buf()
        } else {
            self.project.workspace_root.join(file)
        };
        if self.opened.insert(file.clone()) {
            self.client.open(&file).await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn symbols(&mut self, query: &str) -> Result<SymbolSearchResult> {
        let leaf = query.rsplit("::").next().unwrap_or(query);
        let mut symbols = self.workspace_symbols(leaf).await?;
        debug_assert!(enrichment_files(&symbols, query, LookupPurpose::List).is_empty());
        if query.contains("::") && !self.is_ready() {
            self.wait_ready().await?;
            symbols = self.workspace_symbols(leaf).await?;
        }
        let needle = leaf.to_lowercase();
        symbols.retain(|symbol| symbol.name.to_lowercase().contains(&needle));
        if query.contains("::") {
            let files = qualified_candidate_files(&symbols, query);
            let enriched = if files.is_empty() {
                self.all_document_symbols().await?
            } else {
                self.document_symbols_for_files(&files).await?
            };
            symbols = best_qualified_symbols(enriched, query);
        }
        sort_and_dedup_symbols(&mut symbols, query);
        let truncated = symbols.len() > SYMBOL_SEARCH_LIMIT;
        symbols.truncate(SYMBOL_SEARCH_LIMIT);
        Ok(SymbolSearchResult {
            items: symbols,
            truncated,
        })
    }

    async fn workspace_symbols(&self, query: &str) -> Result<Vec<Symbol>> {
        #[allow(deprecated)]
        let mut values: Vec<SymbolInformation> = self
            .client
            .request::<_, Option<Vec<SymbolInformation>>>(
                "workspace/symbol",
                json!({"query": query}),
            )
            .await?
            .unwrap_or_default();
        for delay in [50, 100, 150, 200, 250, 300, 350, 400] {
            if !values.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(delay)).await;
            values = self
                .client
                .request::<_, Option<Vec<SymbolInformation>>>(
                    "workspace/symbol",
                    json!({"query": query}),
                )
                .await?
                .unwrap_or_default();
        }
        Ok(values
            .into_iter()
            .filter_map(|value| self.normalize_symbol(value).ok())
            .collect())
    }

    pub async fn find_symbol(&mut self, query: &str) -> Result<Symbol> {
        let leaf = query.rsplit("::").next().unwrap_or(query);
        let mut lightweight = self.workspace_symbols(leaf).await?;
        if needs_authoritative_retry(
            self.is_ready(),
            provisional_match_count(&lightweight, query),
        ) {
            self.wait_ready().await?;
            lightweight = self.workspace_symbols(leaf).await?;
        }
        let files = resolution_files(&lightweight, query);
        let mut candidates: Vec<_> = lightweight
            .iter()
            .filter(|symbol| symbol.name == leaf || symbol.name == query)
            .filter(|symbol| !query.contains("::") || qualification_matches(symbol, query))
            .cloned()
            .collect();
        let mut candidates_are_enriched = false;

        if query.contains("::") {
            let enriched =
                best_qualified_symbols(self.document_symbols_for_files(&files).await?, query);
            if !enriched.is_empty() {
                candidates = enriched;
                candidates_are_enriched = true;
            }
        } else if candidates.is_empty() && !files.is_empty() {
            let mut enriched = self.document_symbols_for_files(&files).await?;
            enriched.retain(|symbol| symbol.name == leaf);
            if !enriched.is_empty() {
                candidates = enriched;
                candidates_are_enriched = true;
            }
        }
        sort_and_dedup_symbols(&mut candidates, query);

        if candidates.is_empty() {
            let mut fallback = self.all_document_symbols().await?;
            if query.contains("::") {
                fallback = best_qualified_symbols(fallback, query);
            } else {
                fallback.retain(|symbol| symbol.name == leaf);
            }
            return select_symbol(fallback, query);
        }

        if candidates.len() == 1 {
            let candidate = candidates.pop().unwrap();
            if candidates_are_enriched {
                return Ok(candidate);
            }
            let enriched = self
                .document_symbols_for_files(std::slice::from_ref(&candidate.file))
                .await?;
            return Ok(enriched
                .into_iter()
                .find(|symbol| {
                    symbol.name == candidate.name
                        && symbol.selection_range.start == candidate.selection_range.start
                })
                .unwrap_or(candidate));
        }
        select_symbol(candidates, query)
    }

    pub async fn all_document_symbols(&mut self) -> Result<Vec<Symbol>> {
        let files = self.project.rust_files.clone();
        self.document_symbols_for_files(&files).await
    }

    async fn document_symbols_for_files(&self, files: &[PathBuf]) -> Result<Vec<Symbol>> {
        let mut result = Vec::new();
        for file in files {
            let absolute = self.absolute(file);
            let response: Option<DocumentSymbolResponse> = self
                .client
                .request(
                    "textDocument/documentSymbol",
                    json!({"textDocument": {"uri": path_uri(&absolute, false)?}}),
                )
                .await?;
            match response {
                Some(DocumentSymbolResponse::Nested(symbols)) => {
                    for symbol in symbols {
                        self.flatten_document_symbol(&absolute, symbol, None, &mut result);
                    }
                }
                #[allow(deprecated)]
                Some(DocumentSymbolResponse::Flat(symbols)) => {
                    for symbol in symbols {
                        if let Ok(symbol) = self.normalize_symbol(symbol) {
                            result.push(symbol);
                        }
                    }
                }
                None => {}
            }
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        result.dedup_by(|a, b| a.id == b.id);
        Ok(result)
    }

    pub async fn references(&mut self, symbol: &Symbol) -> Result<Vec<Location>> {
        let values: Option<Vec<LspLocation>> = self.client.request(
            "textDocument/references",
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.selection_range.start), "context": {"includeDeclaration": true}}),
        ).await?;
        self.normalize_locations(values.unwrap_or_default())
    }

    pub async fn definition(&mut self, symbol: &Symbol) -> Result<Option<Location>> {
        let value: Option<GotoDefinitionResponse> = self.client.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.selection_range.start)}),
        ).await?;
        let locations = match value {
            Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
            Some(GotoDefinitionResponse::Array(locations)) => locations,
            Some(GotoDefinitionResponse::Link(links)) => links
                .into_iter()
                .map(|link| LspLocation {
                    uri: link.target_uri,
                    range: link.target_selection_range,
                })
                .collect(),
            None => Vec::new(),
        };
        Ok(self.normalize_locations(locations)?.into_iter().next())
    }

    pub async fn hover(&mut self, symbol: &Symbol) -> Result<Option<String>> {
        let hover: Option<Hover> = self.client.request(
            "textDocument/hover",
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.selection_range.start)}),
        ).await?;
        Ok(hover.map(|hover| hover_text(hover.contents)))
    }

    pub async fn calls(&mut self, symbol: &Symbol, depth: u8, incoming: bool) -> Result<CallTree> {
        let items: Option<Vec<CallHierarchyItem>> = self.client.request(
            "textDocument/prepareCallHierarchy",
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.selection_range.start)}),
        ).await?;
        let mut nodes = Vec::new();
        let mut traversal = TraversalState {
            remaining: CALL_NODE_BUDGET,
            truncated: false,
        };
        for item in items.unwrap_or_default() {
            let mut path = HashSet::from([call_item_key(&item)]);
            nodes.extend(
                self.expand_calls(item, depth, incoming, &mut path, &mut traversal)
                    .await?,
            );
        }
        nodes.sort_by(|a, b| a.name.cmp(&b.name).then(a.location.cmp(&b.location)));
        nodes.dedup_by(|a, b| a.name == b.name && a.location == b.location);
        Ok(CallTree {
            nodes,
            truncated: traversal.truncated,
        })
    }

    pub async fn callers_for_target(&self, target: &CallTarget) -> Result<Option<CallTargets>> {
        self.call_targets(target, true).await
    }

    pub async fn callees_for_target(&self, target: &CallTarget) -> Result<Option<CallTargets>> {
        self.call_targets(target, false).await
    }

    async fn call_targets(
        &self,
        target: &CallTarget,
        incoming: bool,
    ) -> Result<Option<CallTargets>> {
        let Some(item) = self.prepare_call_item(target).await? else {
            return Ok(None);
        };
        let items: Vec<CallHierarchyItem> = if incoming {
            let calls: Option<Vec<CallHierarchyIncomingCall>> = self
                .client
                .request("callHierarchy/incomingCalls", json!({"item": item}))
                .await?;
            calls
                .unwrap_or_default()
                .into_iter()
                .map(|call| call.from)
                .collect()
        } else {
            let calls: Option<Vec<CallHierarchyOutgoingCall>> = self
                .client
                .request("callHierarchy/outgoingCalls", json!({"item": item}))
                .await?;
            calls
                .unwrap_or_default()
                .into_iter()
                .map(|call| call.to)
                .collect()
        };
        let mut items = items
            .into_iter()
            .map(|item| self.normalize_call_target(&item))
            .collect::<Result<Vec<_>>>()?;
        items.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.file.cmp(&b.file))
                .then(a.selection_range.cmp(&b.selection_range))
        });
        items.dedup_by(|a, b| {
            a.name == b.name && a.file == b.file && a.selection_range == b.selection_range
        });
        let truncated = items.len() > CALL_CHILD_LIMIT;
        items.truncate(CALL_CHILD_LIMIT);
        Ok(Some(CallTargets { items, truncated }))
    }

    async fn prepare_call_item(&self, target: &CallTarget) -> Result<Option<CallHierarchyItem>> {
        let items: Option<Vec<CallHierarchyItem>> = self
            .client
            .request(
                "textDocument/prepareCallHierarchy",
                json!({
                    "textDocument": {"uri": path_uri(&self.absolute(&target.file), false)?},
                    "position": lsp_position(target.selection_range.start)
                }),
            )
            .await?;
        let mut items = items.unwrap_or_default();
        items.sort_by_key(call_item_key);
        if let Some(index) = items.iter().position(|item| {
            self.normalize_call_target(item).is_ok_and(|candidate| {
                candidate.file == target.file
                    && candidate.selection_range == target.selection_range
                    && candidate.name == target.name
            })
        }) {
            return Ok(Some(items.remove(index)));
        }
        Ok(items.into_iter().next())
    }

    fn normalize_call_target(&self, item: &CallHierarchyItem) -> Result<CallTarget> {
        Ok(CallTarget {
            name: item.name.clone(),
            kind: kind_name(item.kind).into(),
            file: self.relative(uri_path(item.uri.as_str())?),
            range: range(item.range),
            selection_range: range(item.selection_range),
            detail: item.detail.clone(),
        })
    }

    fn expand_calls<'a>(
        &'a self,
        item: CallHierarchyItem,
        depth: u8,
        incoming: bool,
        path: &'a mut HashSet<String>,
        traversal: &'a mut TraversalState,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CallNode>>> + Send + 'a>> {
        Box::pin(async move {
            if depth == 0 {
                return Ok(Vec::new());
            }
            let items: Vec<CallHierarchyItem> = if incoming {
                let calls: Option<Vec<CallHierarchyIncomingCall>> = self
                    .client
                    .request("callHierarchy/incomingCalls", json!({"item": item}))
                    .await?;
                calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|call| call.from)
                    .collect()
            } else {
                let calls: Option<Vec<CallHierarchyOutgoingCall>> = self
                    .client
                    .request("callHierarchy/outgoingCalls", json!({"item": item}))
                    .await?;
                calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|call| call.to)
                    .collect()
            };
            let mut calls: Vec<_> = items
                .into_iter()
                .map(|item| {
                    let target = self.normalize_call_target(&item)?;
                    Ok(NormalizedCall {
                        key: call_item_key(&item),
                        name: item.name.clone(),
                        item,
                        location: Location {
                            file: target.file,
                            range: target.selection_range,
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            calls.sort_by(|a, b| a.name.cmp(&b.name).then(a.location.cmp(&b.location)));
            calls.dedup_by(|a, b| a.name == b.name && a.location == b.location);
            if calls.len() > CALL_CHILD_LIMIT {
                traversal.truncated = true;
                calls.truncate(CALL_CHILD_LIMIT);
            }

            let mut nodes = Vec::new();
            for call in calls {
                if traversal.remaining == 0 {
                    traversal.truncated = true;
                    break;
                }
                traversal.remaining -= 1;
                let cycle = path.contains(&call.key);
                let children = if cycle || depth == 1 {
                    Vec::new()
                } else {
                    path.insert(call.key.clone());
                    let children = self
                        .expand_calls(call.item, depth - 1, incoming, path, traversal)
                        .await?;
                    path.remove(&call.key);
                    children
                };
                nodes.push(CallNode {
                    name: call.name,
                    location: call.location,
                    children,
                    cycle,
                });
            }
            Ok(nodes)
        })
    }

    pub async fn diagnostics(&mut self) -> Result<Vec<Diagnostic>> {
        self.wait_ready().await?;
        let generation = self.client.diagnostic_generation();
        let mut expected = HashSet::new();
        let mut opened = false;
        for file in self.project.rust_files.clone() {
            expected.insert(path_uri(&file, false)?);
            opened |= self.open(&file).await?;
        }
        let after = if opened {
            generation
        } else {
            generation.saturating_sub(1)
        };
        self.client
            .wait_for_diagnostics(&expected, after, Duration::from_secs(30), true)
            .await?;
        self.collect_diagnostics(&expected)
    }

    pub async fn diagnostics_for_file(&mut self, file: &Path) -> Result<Vec<Diagnostic>> {
        let absolute = self.absolute(file);
        let uri = path_uri(&absolute, false)?;
        let generation = self.client.diagnostic_generation();
        let opened = self.open(&absolute).await?;
        let expected = HashSet::from([uri]);
        let after = if opened {
            generation
        } else {
            generation.saturating_sub(1)
        };
        self.client
            .wait_for_diagnostics(&expected, after, Duration::from_secs(15), false)
            .await?;
        self.collect_diagnostics(&expected)
    }

    fn collect_diagnostics(&self, expected: &HashSet<String>) -> Result<Vec<Diagnostic>> {
        let mut diagnostics = Vec::new();
        for (uri, diagnostic) in self.client.diagnostics() {
            if !expected.contains(&uri) {
                continue;
            }
            let severity = match diagnostic.severity {
                Some(lsp_types::DiagnosticSeverity::ERROR) => Severity::Error,
                Some(lsp_types::DiagnosticSeverity::WARNING) => Severity::Warning,
                Some(lsp_types::DiagnosticSeverity::INFORMATION) => Severity::Information,
                _ => Severity::Hint,
            };
            diagnostics.push(Diagnostic {
                file: self.relative(uri_path(&uri)?),
                range: range(diagnostic.range),
                severity,
                message: diagnostic.message,
            });
        }
        diagnostics.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.range.cmp(&b.range))
                .then(a.severity.cmp(&b.severity))
                .then(a.message.cmp(&b.message))
        });
        Ok(diagnostics)
    }

    pub async fn explain(&mut self, symbol: Symbol) -> Result<Explanation> {
        let references = self.references(&symbol).await?;
        let definition = self.definition(&symbol).await?;
        let hover = self.hover(&symbol).await?;
        let callers = self.calls(&symbol, 1, true).await?;
        let callees = self.calls(&symbol, 1, false).await?;
        let diagnostics = self
            .diagnostics_for_file(&symbol.file)
            .await?
            .into_iter()
            .filter(|diagnostic| {
                diagnostic.file == symbol.file && overlaps(diagnostic.range, symbol.range)
            })
            .collect();
        let source = source_fragment(&self.absolute(&symbol.file), symbol.selection_range)?;
        let signature = hover.as_deref().and_then(signature_line).map(str::to_owned);
        Ok(Explanation {
            symbol,
            signature,
            hover,
            definition,
            source,
            callers,
            callees,
            references: references.len(),
            diagnostics,
        })
    }

    pub fn source_preview_for_symbol(&self, symbol: &Symbol) -> Result<String> {
        source_preview(&self.absolute(&symbol.file), symbol.selection_range)
    }

    pub async fn shutdown(&mut self) {
        self.client.shutdown().await;
    }

    fn normalize_symbol(&self, value: SymbolInformation) -> Result<Symbol> {
        let file = self.relative(uri_path(&value.location.uri.to_string())?);
        let mut symbol = Symbol {
            id: String::new(),
            name: value.name,
            kind: kind_name(value.kind).into(),
            file,
            range: range(value.location.range),
            selection_range: range(value.location.range),
            container: value.container_name,
        };
        symbol.id = stable_symbol_id(&self.project.workspace_root, &symbol);
        Ok(symbol)
    }

    fn flatten_document_symbol(
        &self,
        file: &Path,
        value: DocumentSymbol,
        parent: Option<String>,
        output: &mut Vec<Symbol>,
    ) {
        let name = value.name;
        let container = parent.clone();
        let path = match &parent {
            Some(parent) => format!("{parent}::{name}"),
            None => name.clone(),
        };
        let relative = self.relative(file.to_path_buf());
        let mut symbol = Symbol {
            id: String::new(),
            name: name.clone(),
            kind: kind_name(value.kind).into(),
            file: relative,
            range: range(value.range),
            selection_range: range(value.selection_range),
            container,
        };
        symbol.id = stable_symbol_id(&self.project.workspace_root, &symbol);
        output.push(symbol);
        for child in value.children.unwrap_or_default() {
            self.flatten_document_symbol(file, child, Some(path.clone()), output);
        }
    }

    fn normalize_locations(&self, values: Vec<LspLocation>) -> Result<Vec<Location>> {
        let mut locations: Vec<_> = values
            .into_iter()
            .map(|value| self.normalize_location(value))
            .collect::<Result<_>>()?;
        locations.sort();
        locations.dedup();
        Ok(locations)
    }

    fn normalize_location(&self, value: LspLocation) -> Result<Location> {
        Ok(Location {
            file: self.relative(uri_path(&value.uri.to_string())?),
            range: range(value.range),
        })
    }

    fn relative(&self, path: PathBuf) -> PathBuf {
        path.strip_prefix(&self.project.workspace_root)
            .unwrap_or(&path)
            .to_path_buf()
    }
    fn absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project.workspace_root.join(path)
        }
    }
}

fn select_symbol(mut candidates: Vec<Symbol>, query: &str) -> Result<Symbol> {
    sort_and_dedup_symbols(&mut candidates, query);
    match candidates.len() {
        0 => Err(ResolveError::NotFound {
            query: query.to_owned(),
        }
        .into()),
        1 => Ok(candidates.pop().expect("one candidate")),
        _ => {
            let truncated = candidates.len() > AMBIGUITY_LIMIT;
            candidates.truncate(AMBIGUITY_LIMIT);
            Err(ResolveError::Ambiguous {
                query: query.to_owned(),
                candidates,
                truncated,
            }
            .into())
        }
    }
}

fn qualification_matches(symbol: &Symbol, query: &str) -> bool {
    let Some((qualifier, _)) = query.rsplit_once("::") else {
        return true;
    };
    symbol.container.as_deref().is_some_and(|container| {
        let container = normalized_container(container);
        container == qualifier || container.ends_with(&format!("::{qualifier}"))
    }) || symbol.name == query
}

fn best_qualified_symbols(symbols: Vec<Symbol>, query: &str) -> Vec<Symbol> {
    let Some((qualifier, leaf)) = query.rsplit_once("::") else {
        return symbols;
    };
    let mut exact = Vec::new();
    let mut suffix = Vec::new();
    for symbol in symbols.into_iter().filter(|symbol| symbol.name == leaf) {
        let Some(container) = symbol.container.as_deref() else {
            continue;
        };
        let container = normalized_container(container);
        if container == qualifier {
            exact.push(symbol);
        } else if container.ends_with(&format!("::{qualifier}")) {
            suffix.push(symbol);
        }
    }
    if exact.is_empty() {
        suffix
    } else {
        exact
    }
}

fn normalized_container(container: &str) -> String {
    let container = container.trim();
    if let Some(target) = normalize_impl_target(container) {
        return target.to_owned();
    }
    if let Some((prefix, implementation)) = container.rsplit_once("::impl") {
        if let Some(target) = normalize_impl_target(&format!("impl{implementation}")) {
            return format!("{prefix}::{target}");
        }
    }
    container.to_owned()
}

fn normalize_impl_target(container: &str) -> Option<&str> {
    let mut value = container.trim();
    if !value.starts_with("impl") {
        return None;
    }
    value = &value[4..];
    value = value.trim_start();
    if value.starts_with('<') {
        let mut depth = 0_u32;
        let mut end = None;
        for (index, character) in value.char_indices() {
            match character {
                '<' => depth += 1,
                '>' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(index + character.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }
        value = value.get(end?..)?.trim_start();
    }
    if let Some((_, target)) = value.rsplit_once(" for ") {
        value = target.trim();
    }
    let end = value
        .char_indices()
        .find_map(|(index, character)| matches!(character, '<' | ' ' | '{').then_some(index))
        .unwrap_or(value.len());
    let base = value[..end].trim();
    base.rsplit("::").next().filter(|name| !name.is_empty())
}

fn qualified_name(symbol: &Symbol) -> String {
    match symbol.container.as_deref() {
        Some(container) => format!("{}::{}", normalized_container(container), symbol.name),
        None => symbol.name.clone(),
    }
}

fn symbol_score(symbol: &Symbol, query: &str) -> u8 {
    let leaf = query.rsplit("::").next().unwrap_or(query);
    u8::from(symbol.name == query) * 4
        + u8::from(symbol.name == leaf) * 2
        + u8::from(qualification_matches(symbol, query))
}

fn range_size(range: Range) -> (u32, u32) {
    (
        range.end.line.saturating_sub(range.start.line),
        range.end.character.saturating_sub(range.start.character),
    )
}

fn sort_and_dedup_symbols(symbols: &mut Vec<Symbol>, query: &str) {
    symbols.sort_by(|a, b| {
        symbol_score(b, query)
            .cmp(&symbol_score(a, query))
            .then(range_size(b.range).cmp(&range_size(a.range)))
            .then(a.id.cmp(&b.id))
    });
    symbols.dedup_by(|a, b| {
        a.file == b.file && a.selection_range.start == b.selection_range.start && a.name == b.name
    });
}

fn enrichment_files(symbols: &[Symbol], leaf: &str, purpose: LookupPurpose) -> Vec<PathBuf> {
    if matches!(purpose, LookupPurpose::List) {
        return Vec::new();
    }
    let mut files: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.name == leaf)
        .map(|symbol| symbol.file.clone())
        .collect();
    if files.is_empty() {
        files.extend(symbols.iter().map(|symbol| symbol.file.clone()));
    }
    files.sort();
    files.dedup();
    files
}

fn qualified_candidate_files(symbols: &[Symbol], query: &str) -> Vec<PathBuf> {
    let leaf = query.rsplit("::").next().unwrap_or(query);
    let exact: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.name == leaf)
        .collect();
    let qualified: Vec<_> = exact
        .iter()
        .copied()
        .filter(|symbol| qualification_matches(symbol, query))
        .collect();
    let selected = if qualified.is_empty() {
        exact
    } else {
        qualified
    };
    let mut files: Vec<_> = selected
        .into_iter()
        .map(|symbol| symbol.file.clone())
        .collect();
    files.sort();
    files.dedup();
    files
}

fn resolution_files(symbols: &[Symbol], query: &str) -> Vec<PathBuf> {
    if query.contains("::") {
        qualified_candidate_files(symbols, query)
    } else {
        enrichment_files(symbols, query, LookupPurpose::Resolve)
    }
}

fn provisional_match_count(symbols: &[Symbol], query: &str) -> usize {
    let leaf = query.rsplit("::").next().unwrap_or(query);
    symbols
        .iter()
        .filter(|symbol| symbol.name == leaf)
        .filter(|symbol| !query.contains("::") || qualification_matches(symbol, query))
        .count()
}

fn needs_authoritative_retry(ready: bool, provisional_matches: usize) -> bool {
    !ready && provisional_matches <= 1
}

fn call_item_key(item: &CallHierarchyItem) -> String {
    format!(
        "{}:{}:{}",
        item.uri.as_str(),
        item.selection_range.start.line,
        item.selection_range.start.character
    )
}

fn lsp_position(position: Position) -> Value {
    json!({"line": position.line, "character": position.character})
}
fn range(value: lsp_types::Range) -> Range {
    Range {
        start: Position {
            line: value.start.line,
            character: value.start.character,
        },
        end: Position {
            line: value.end.line,
            character: value.end.character,
        },
    }
}

fn kind_name(kind: lsp_types::SymbolKind) -> &'static str {
    match kind {
        lsp_types::SymbolKind::FILE => "file",
        lsp_types::SymbolKind::MODULE => "module",
        lsp_types::SymbolKind::NAMESPACE => "namespace",
        lsp_types::SymbolKind::PACKAGE => "package",
        lsp_types::SymbolKind::CLASS => "class",
        lsp_types::SymbolKind::METHOD => "method",
        lsp_types::SymbolKind::FUNCTION => "function",
        lsp_types::SymbolKind::CONSTRUCTOR => "constructor",
        lsp_types::SymbolKind::FIELD => "field",
        lsp_types::SymbolKind::VARIABLE => "variable",
        lsp_types::SymbolKind::CONSTANT => "constant",
        lsp_types::SymbolKind::STRING => "string",
        lsp_types::SymbolKind::NUMBER => "number",
        lsp_types::SymbolKind::BOOLEAN => "boolean",
        lsp_types::SymbolKind::ARRAY => "array",
        lsp_types::SymbolKind::OBJECT => "object",
        lsp_types::SymbolKind::ENUM => "enum",
        lsp_types::SymbolKind::INTERFACE => "trait",
        lsp_types::SymbolKind::STRUCT => "struct",
        lsp_types::SymbolKind::EVENT => "event",
        lsp_types::SymbolKind::OPERATOR => "operator",
        lsp_types::SymbolKind::TYPE_PARAMETER => "type_parameter",
        _ => "other",
    }
}

fn hover_text(contents: HoverContents) -> String {
    match contents {
        HoverContents::Scalar(value) => marked(value),
        HoverContents::Array(values) => values
            .into_iter()
            .map(marked)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(value) => value.value,
    }
}

fn marked(value: MarkedString) -> String {
    match value {
        MarkedString::String(value) => value,
        MarkedString::LanguageString(value) => value.value,
    }
}
fn signature_line(hover: &str) -> Option<&str> {
    hover.lines().find(|line| {
        line.contains("fn ")
            || line.contains("struct ")
            || line.contains("enum ")
            || line.contains("trait ")
    })
}
fn overlaps(a: Range, b: Range) -> bool {
    a.start <= b.end && b.start <= a.end
}

fn source_fragment(file: &Path, selected: Range) -> Result<String> {
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let lines: Vec<_> = source.lines().collect();
    let start = selected.start.line.saturating_sub(2) as usize;
    let end = ((selected.end.line + 3) as usize).min(lines.len());
    Ok(lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>4} | {}", start + index + 1, line))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn source_preview(file: &Path, selected: Range) -> Result<String> {
    let source = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let lines: Vec<_> = source.lines().collect();
    let start = selected.start.line.saturating_sub(10) as usize;
    let end = ((selected.end.line + 11) as usize).min(lines.len());
    Ok(lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let line_number = start + index;
            let marker = if line_number == selected.start.line as usize {
                '>'
            } else {
                ' '
            };
            format!("{marker}{:>4} | {}", line_number + 1, line)
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn require_analyzer() -> bool {
        let available = Command::new("rust-analyzer")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !available {
            assert!(
                std::env::var_os("CI").is_none(),
                "rust-analyzer is required in CI"
            );
            eprintln!("skipping rust-analyzer test: install rust-analyzer");
        }
        available
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    async fn fixture_analyzer(name: &str) -> RustAnalyzer {
        let project = Project::discover(&fixture(name)).expect("fixture should be a Cargo project");
        let analyzer = RustAnalyzer::start(project, Path::new("rust-analyzer"))
            .await
            .expect("rust-analyzer should start");
        analyzer.wait_ready().await.expect("analyzer should settle");
        analyzer
    }

    #[test]
    fn normalizes_kind_names() {
        assert_eq!(kind_name(lsp_types::SymbolKind::STRUCT), "struct");
    }

    #[test]
    fn extracts_signature_from_hover() {
        assert_eq!(
            signature_line("```rust\nfn add(a: i32) -> i32\n```"),
            Some("fn add(a: i32) -> i32")
        );
    }

    #[test]
    fn source_preview_is_bounded_and_marks_the_selected_line() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("preview.rs");
        let source = (1..=40)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file, source).unwrap();
        let preview = source_preview(
            &file,
            Range {
                start: Position {
                    line: 19,
                    character: 0,
                },
                end: Position {
                    line: 19,
                    character: 7,
                },
            },
        )
        .unwrap();
        assert_eq!(preview.lines().count(), 21);
        assert!(preview.lines().any(|line| line == ">  20 | line 20"));
    }

    #[test]
    fn qualification_is_segment_exact() {
        let symbol = Symbol {
            id: "id".into(),
            name: "run".into(),
            kind: "method".into(),
            file: "src/main.rs".into(),
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 1,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 0,
                    character: 3,
                },
                end: Position {
                    line: 0,
                    character: 6,
                },
            },
            container: Some("impl MyEngine".into()),
        };
        assert!(qualification_matches(&symbol, "MyEngine::run"));
        assert!(!qualification_matches(&symbol, "Engine::run"));
    }

    #[test]
    fn normalizes_generic_impl_targets() {
        for input in [
            "impl Engine",
            "impl Engine<T>",
            "impl<T> Engine<T>",
            "impl<T, U> Engine<T, U>",
            "impl Trait for Engine",
            "impl<T> Trait<T> for Engine<T>",
            "impl<'a, T> Engine<'a, T>",
        ] {
            assert_eq!(normalize_impl_target(input), Some("Engine"), "{input}");
        }
        assert_eq!(
            normalized_container("api::impl<T> Engine<T>"),
            "api::Engine"
        );
        assert_eq!(
            normalized_container("api::impl<T> Runner<T> for Engine<T>"),
            "api::Engine"
        );
    }

    #[test]
    fn lookup_strategy_never_scans_workspace_for_symbol_lists() {
        let mut symbols = Vec::new();
        for index in 0..300 {
            symbols.push(Symbol {
                id: index.to_string(),
                name: if index == 173 {
                    "UniqueName".into()
                } else {
                    format!("Other{index}")
                },
                kind: "function".into(),
                file: format!("src/file_{index}.rs").into(),
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 1,
                        character: 0,
                    },
                },
                selection_range: Range {
                    start: Position {
                        line: 0,
                        character: 3,
                    },
                    end: Position {
                        line: 0,
                        character: 13,
                    },
                },
                container: None,
            });
        }
        assert!(enrichment_files(&symbols, "UniqueName", LookupPurpose::List).is_empty());
        assert_eq!(
            enrichment_files(&symbols, "UniqueName", LookupPurpose::Resolve),
            vec![PathBuf::from("src/file_173.rs")]
        );
    }

    #[test]
    fn authoritative_lookup_retries_partial_or_negative_results() {
        assert!(needs_authoritative_retry(false, 0));
        assert!(needs_authoritative_retry(false, 1));
        assert!(!needs_authoritative_retry(false, 2));
        assert!(!needs_authoritative_retry(true, 0));
    }

    #[tokio::test]
    async fn one_hop_calls_are_direct_and_follow_exact_locations() {
        if !require_analyzer() {
            return;
        }
        let mut analyzer = fixture_analyzer("simple_project").await;

        let a = analyzer.find_symbol("chain_a").await.unwrap();
        let b = analyzer.find_symbol("chain_b").await.unwrap();
        let c = analyzer.find_symbol("chain_c").await.unwrap();
        let a_callees = analyzer
            .callees_for_target(&CallTarget::from_symbol(&a))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(a_callees.items.len(), 1);
        assert_eq!(a_callees.items[0].selection_range, b.selection_range);
        let b_callers = analyzer
            .callers_for_target(&CallTarget::from_symbol(&b))
            .await
            .unwrap()
            .unwrap();
        assert!(b_callers
            .items
            .iter()
            .any(|target| target.selection_range == a.selection_range));
        let b_callees = analyzer
            .callees_for_target(&a_callees.items[0])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(b_callees.items.len(), 1);
        assert_eq!(b_callees.items[0].selection_range, c.selection_range);

        let first = analyzer.find_symbol("first::run").await.unwrap();
        let second = analyzer.find_symbol("second::run").await.unwrap();
        let normalized = CallTarget::from_symbol(&first);
        assert_eq!(normalized.name, "run");
        assert_eq!(normalized.kind, "function");
        assert_eq!(normalized.file, PathBuf::from("src/main.rs"));
        assert_eq!(normalized.selection_range, first.selection_range);
        let callers = analyzer
            .callers_for_target(&normalized)
            .await
            .unwrap()
            .unwrap();
        let caller = callers
            .items
            .iter()
            .find(|target| target.name == "invoke_first")
            .expect("first::run should have its exact caller");
        let followed = analyzer.callees_for_target(caller).await.unwrap().unwrap();
        assert!(followed
            .items
            .iter()
            .any(|target| target.selection_range == first.selection_range));
        assert!(!followed
            .items
            .iter()
            .any(|target| target.selection_range == second.selection_range));
        assert!(followed
            .items
            .iter()
            .all(|target| !target.file.is_absolute()));

        analyzer.shutdown().await;
    }

    #[tokio::test]
    async fn one_hop_calls_handle_direct_recursion_without_expanding_it() {
        if !require_analyzer() {
            return;
        }
        let mut analyzer = fixture_analyzer("recursive_project").await;
        let recurse = analyzer.find_symbol("recurse").await.unwrap();
        let target = CallTarget::from_symbol(&recurse);
        let callers = analyzer.callers_for_target(&target).await.unwrap().unwrap();
        let callees = analyzer.callees_for_target(&target).await.unwrap().unwrap();
        assert!(callers.items.iter().any(|item| item.name == "recurse"));
        assert!(callees.items.iter().any(|item| item.name == "recurse"));
        assert!(callers.items.len() <= CALL_CHILD_LIMIT);
        assert!(callees.items.len() <= CALL_CHILD_LIMIT);
        analyzer.shutdown().await;
    }
}
