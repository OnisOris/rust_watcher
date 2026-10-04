use crate::lsp::{path_uri, uri_path, LspClient};
use crate::model::{
    stable_symbol_id, CallNode, Diagnostic, Explanation, Location, Position, Range, Severity,
    Symbol,
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

pub struct RustAnalyzer {
    pub project: Project,
    client: LspClient,
    opened: HashSet<PathBuf>,
}

impl RustAnalyzer {
    pub async fn start(project: Project, binary: &Path) -> Result<Self> {
        let client = LspClient::start(binary, &project.workspace_root).await?;
        client.wait_ready(Duration::from_secs(30)).await?;
        Ok(Self {
            project,
            client,
            opened: HashSet::new(),
        })
    }

    async fn open(&mut self, file: &Path) -> Result<()> {
        let file = if file.is_absolute() {
            file.to_path_buf()
        } else {
            self.project.workspace_root.join(file)
        };
        if self.opened.insert(file.clone()) {
            self.client.open(&file).await?;
        }
        Ok(())
    }

    pub async fn symbols(&mut self, query: &str) -> Result<Vec<Symbol>> {
        #[allow(deprecated)]
        let values: Vec<SymbolInformation> = self
            .client
            .request(
                "workspace/symbol",
                json!({"query": query.rsplit("::").next().unwrap_or(query)}),
            )
            .await?;
        let mut symbols: Vec<_> = values
            .into_iter()
            .filter_map(|value| self.normalize_symbol(value).ok())
            .collect();
        if query.contains("::") || symbols.is_empty() {
            symbols.extend(self.all_document_symbols().await?);
        }
        let needle = query.to_lowercase();
        symbols.retain(|symbol| {
            symbol
                .name
                .to_lowercase()
                .contains(needle.rsplit("::").next().unwrap_or(&needle))
                && qualification_matches(symbol, query)
        });
        symbols.sort_by(|a, b| {
            symbol_score(b, query)
                .cmp(&symbol_score(a, query))
                .then(a.id.cmp(&b.id))
        });
        symbols.dedup_by(|a, b| a.id == b.id);
        Ok(symbols)
    }

    pub async fn find_symbol(&mut self, query: &str) -> Result<Symbol> {
        self.symbols(query)
            .await?
            .into_iter()
            .next()
            .with_context(|| format!("symbol not found: {query}"))
    }

    pub async fn all_document_symbols(&mut self) -> Result<Vec<Symbol>> {
        let files = self.project.rust_files.clone();
        let mut result = Vec::new();
        for file in files {
            let response: Option<DocumentSymbolResponse> = self
                .client
                .request(
                    "textDocument/documentSymbol",
                    json!({"textDocument": {"uri": path_uri(&file, false)?}}),
                )
                .await?;
            match response {
                Some(DocumentSymbolResponse::Nested(symbols)) => {
                    for symbol in symbols {
                        self.flatten_document_symbol(&file, symbol, None, &mut result);
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
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.range.start), "context": {"includeDeclaration": true}}),
        ).await?;
        self.normalize_locations(values.unwrap_or_default())
    }

    pub async fn definition(&mut self, symbol: &Symbol) -> Result<Option<Location>> {
        let value: Option<GotoDefinitionResponse> = self.client.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.range.start)}),
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
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.range.start)}),
        ).await?;
        Ok(hover.map(|hover| hover_text(hover.contents)))
    }

    pub async fn calls(
        &mut self,
        symbol: &Symbol,
        depth: u8,
        incoming: bool,
    ) -> Result<Vec<CallNode>> {
        let items: Option<Vec<CallHierarchyItem>> = self.client.request(
            "textDocument/prepareCallHierarchy",
            json!({"textDocument": {"uri": path_uri(&self.absolute(&symbol.file), false)?}, "position": lsp_position(symbol.range.start)}),
        ).await?;
        let mut result = Vec::new();
        for item in items.unwrap_or_default() {
            result.extend(self.expand_calls(item, depth, incoming).await?);
        }
        result.sort_by(|a, b| a.name.cmp(&b.name).then(a.location.cmp(&b.location)));
        Ok(result)
    }

    fn expand_calls<'a>(
        &'a self,
        item: CallHierarchyItem,
        depth: u8,
        incoming: bool,
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
            let mut nodes = Vec::new();
            for (child, location) in pairs.into_iter().take(20) {
                let normalized = self.normalize_location(location)?;
                let children = self
                    .expand_calls(child.clone(), depth - 1, incoming)
                    .await?;
                nodes.push(CallNode {
                    name: child.name,
                    location: normalized,
                    children,
                });
            }
            nodes.sort_by(|a, b| a.name.cmp(&b.name).then(a.location.cmp(&b.location)));
            nodes.dedup_by(|a, b| a.name == b.name && a.location == b.location);
            Ok(nodes)
        })
    }

    pub async fn diagnostics(&mut self) -> Result<Vec<Diagnostic>> {
        let generation = self.client.notification_generation();
        for file in self.project.rust_files.clone() {
            self.open(&file).await?;
        }
        if self.client.notification_generation() == generation {
            self.client
                .wait_for_notifications(generation, Duration::from_secs(15))
                .await?;
        }
        let mut diagnostics = Vec::new();
        for (uri, diagnostic) in self.client.diagnostics() {
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
            .diagnostics()
            .await?
            .into_iter()
            .filter(|diagnostic| {
                diagnostic.file == symbol.file && overlaps(diagnostic.range, symbol.range)
            })
            .collect();
        let source = source_fragment(&self.absolute(&symbol.file), symbol.range)?;
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
        let relative = self.relative(file.to_path_buf());
        let mut symbol = Symbol {
            id: String::new(),
            name: name.clone(),
            kind: kind_name(value.kind).into(),
            file: relative,
            range: range(value.selection_range),
            container,
        };
        symbol.id = stable_symbol_id(&self.project.workspace_root, &symbol);
        output.push(symbol);
        for child in value.children.unwrap_or_default() {
            self.flatten_document_symbol(file, child, Some(name.clone()), output);
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

fn qualification_matches(symbol: &Symbol, query: &str) -> bool {
    let Some((container, _)) = query.rsplit_once("::") else {
        return true;
    };
    symbol
        .container
        .as_deref()
        .is_some_and(|value| value.contains(container))
        || symbol.name == query
}

fn symbol_score(symbol: &Symbol, query: &str) -> u8 {
    let leaf = query.rsplit("::").next().unwrap_or(query);
    u8::from(symbol.name == query) * 4
        + u8::from(symbol.name == leaf) * 2
        + u8::from(qualification_matches(symbol, query))
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
}
