use crate::lsp::{path_uri, uri_path, LspClient};
use crate::model::{
    stable_symbol_id, CallNode, CallTree, Diagnostic, Explanation, Location, Position, Range,
    Severity, Symbol, SymbolSearchResult,
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
        let mut symbols = self.workspace_symbols(query).await?;
        debug_assert!(enrichment_files(&symbols, query, LookupPurpose::List).is_empty());
        let needle = query.to_lowercase();
        symbols.retain(|symbol| symbol.name.to_lowercase().contains(&needle));
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
        let lightweight = self.workspace_symbols(leaf).await?;
        let files = enrichment_files(&lightweight, leaf, LookupPurpose::Resolve);
        let mut candidates: Vec<_> = lightweight
            .iter()
            .filter(|symbol| symbol.name == leaf || symbol.name == query)
            .filter(|symbol| !query.contains("::") || qualification_matches(symbol, query))
            .cloned()
            .collect();
        let mut candidates_are_enriched = false;

        if query.contains("::") {
            let mut enriched = self.document_symbols_for_files(&files).await?;
            enriched.retain(|symbol| symbol.name == leaf && qualification_matches(symbol, query));
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
            fallback.retain(|symbol| {
                symbol.name == leaf
                    && (!query.contains("::") || qualification_matches(symbol, query))
            });
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

    fn expand_calls<'a>(
        &'a self,
        item: CallHierarchyItem,
        depth: u8,
        incoming: bool,
        path: &'a mut HashSet<String>,
        traversal: &'a mut TraversalState,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CallNode>>> + 'a>> {
        Box::pin(async move {
            if depth == 0 {
                return Ok(Vec::new());
            }
            let pairs: Vec<(CallHierarchyItem, LspLocation)> = if incoming {
                let calls: Option<Vec<CallHierarchyIncomingCall>> = self
                    .client
                    .request("callHierarchy/incomingCalls", json!({"item": item}))
                    .await?;
                calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|call| {
                        let location = LspLocation {
                            uri: call.from.uri.clone(),
                            range: call.from.selection_range,
                        };
                        (call.from, location)
                    })
                    .collect()
            } else {
                let calls: Option<Vec<CallHierarchyOutgoingCall>> = self
                    .client
                    .request("callHierarchy/outgoingCalls", json!({"item": item}))
                    .await?;
                calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|call| {
                        let location = LspLocation {
                            uri: call.to.uri.clone(),
                            range: call.to.selection_range,
                        };
                        (call.to, location)
                    })
                    .collect()
            };
            let mut calls: Vec<_> = pairs
                .into_iter()
                .map(|(item, location)| {
                    let location = self.normalize_location(location)?;
                    Ok(NormalizedCall {
                        key: call_item_key(&item),
                        name: item.name.clone(),
                        item,
                        location,
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

    async fn diagnostics_for_file(&mut self, file: &Path) -> Result<Vec<Diagnostic>> {
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

fn normalized_container(container: &str) -> &str {
    normalize_impl_target(container).unwrap_or(container.trim())
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
