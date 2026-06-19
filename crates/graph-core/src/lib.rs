use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LanguageId {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Other(String),
}

impl LanguageId {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Other(language) => language.as_str(),
        }
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<&str> for LanguageId {
    fn from(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "rs" | "rust" => Self::Rust,
            "ts" | "tsx" | "typescript" => Self::TypeScript,
            "js" | "jsx" | "javascript" => Self::JavaScript,
            "py" | "python" => Self::Python,
            other => Self::Other(other.to_string()),
        }
    }
}

impl Serialize for LanguageId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LanguageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from(value.as_str()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub language: LanguageId,
    pub absolute_path: String,
    pub relative_path: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

pub type LspPosition = TextPosition;
pub type LspRange = TextRange;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRecord {
    pub id: String,
    pub language: LanguageId,
    pub file: String,
    pub range: Option<TextRange>,
    pub severity: DiagnosticSeverity,
    pub source: Option<String>,
    pub message: String,
    pub code: Option<String>,
    pub related_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

pub type AnalysisResult<T> = Result<T, String>;

#[derive(Debug, Clone, Copy)]
pub struct AnalysisContext<'a> {
    pub project_root: &'a Path,
    pub files: &'a [SourceFile],
    pub symbols: &'a [SymbolRecord],
    pub graph_nodes: &'a [GraphNode],
    pub graph_edges: &'a [GraphEdge],
}

#[derive(Debug, Clone, Default)]
pub struct AdapterAnalysisResult {
    pub files: Vec<SourceFile>,
    pub symbols: Vec<SymbolRecord>,
    pub edges: Vec<GraphEdge>,
    pub diagnostics: Vec<DiagnosticRecord>,
}

pub trait LanguageAnalyzer {
    fn language_id(&self) -> LanguageId;
    fn supported_extensions(&self) -> &'static [&'static str];
    fn discover_files<'a>(
        &'a self,
        root: &'a Path,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<SourceFile>>> + Send + 'a>>;
    fn symbols<'a>(
        &'a self,
        file: &'a SourceFile,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<SymbolRecord>>> + Send + 'a>>;
    fn edges<'a>(
        &'a self,
        context: &'a AnalysisContext<'a>,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<GraphEdge>>> + Send + 'a>>;
    fn diagnostics<'a>(
        &'a self,
        file: &'a SourceFile,
    ) -> Pin<Box<dyn Future<Output = AnalysisResult<Vec<DiagnosticRecord>>> + Send + 'a>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    File,
    Module,
    Struct,
    Class,
    Enum,
    Trait,
    Impl,
    Function,
    Method,
    Component,
    Hook,
    Interface,
    TypeAlias,
    Endpoint,
    Macro,
    ExternalCrate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    Contains,
    Imports,
    Uses,
    Calls,
    Renders,
    ApiCall,
    EndpointHandler,
    Implements,
    TypeReference,
    DataFlow,
    ModDeclaration,
    ExternalDependency,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeConfidence {
    Exact,
    Semantic,
    SyntaxFallback,
    #[default]
    Heuristic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GraphMode {
    Macro,
    Meso,
    Micro,
    CallFlow,
    DataFlow,
    Traits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState {
    Empty,
    Indexing,
    Normal,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnalyzerStatus {
    Starting,
    Indexing,
    Ready,
    Fallback,
    Stale,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolRecord {
    pub id: String,
    pub node_id: String,
    pub language: LanguageId,
    pub node_type: NodeType,
    pub label: String,
    pub name: String,
    pub kind: SymbolKindName,
    pub file: String,
    pub module: Option<String>,
    #[serde(rename = "crate")]
    pub crate_name: Option<String>,
    pub line: u32,
    pub character: u32,
    pub range: TextRange,
    pub selection_range: TextRange,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolIndex {
    pub symbols: Vec<SymbolRecord>,
    #[serde(skip)]
    by_id: HashMap<String, usize>,
    #[serde(skip)]
    by_language: HashMap<LanguageId, Vec<usize>>,
    #[serde(skip)]
    by_file: HashMap<String, Vec<usize>>,
    #[serde(skip)]
    by_name: HashMap<String, Vec<usize>>,
    #[serde(skip)]
    by_range: HashMap<TextRange, Vec<usize>>,
    #[serde(skip)]
    by_kind: HashMap<SymbolKindName, Vec<usize>>,
}

impl SymbolIndex {
    pub fn new(symbols: Vec<SymbolRecord>) -> Self {
        let mut by_id = HashMap::new();
        let mut by_language: HashMap<LanguageId, Vec<usize>> = HashMap::new();
        let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_range: HashMap<TextRange, Vec<usize>> = HashMap::new();
        let mut by_kind: HashMap<SymbolKindName, Vec<usize>> = HashMap::new();
        for (idx, symbol) in symbols.iter().enumerate() {
            by_id.insert(symbol.id.clone(), idx);
            by_language
                .entry(symbol.language.clone())
                .or_default()
                .push(idx);
            by_file.entry(symbol.file.clone()).or_default().push(idx);
            by_name.entry(symbol.name.clone()).or_default().push(idx);
            by_range.entry(symbol.range).or_default().push(idx);
            by_kind.entry(symbol.kind).or_default().push(idx);
        }
        Self {
            symbols,
            by_id,
            by_language,
            by_file,
            by_name,
            by_range,
            by_kind,
        }
    }

    pub fn from_nodes(nodes: &[GraphNode]) -> Self {
        Self::new(
            nodes
                .iter()
                .filter_map(SymbolRecord::from_node)
                .collect::<Vec<_>>(),
        )
    }

    pub fn get(&self, id: &str) -> Option<&SymbolRecord> {
        self.by_id.get(id).and_then(|idx| self.symbols.get(*idx))
    }

    pub fn find_by_node_id(&self, node_id: &str) -> Option<&SymbolRecord> {
        self.get(node_id)
    }

    pub fn find_by_language(&self, language: &LanguageId) -> Vec<&SymbolRecord> {
        self.records_for_indices(self.by_language.get(language))
    }

    pub fn find_by_file(&self, file: &str) -> Vec<&SymbolRecord> {
        self.records_for_indices(self.by_file.get(file))
    }

    pub fn find_by_name(&self, name: &str) -> Vec<&SymbolRecord> {
        self.records_for_indices(self.by_name.get(name))
    }

    pub fn find_by_range(&self, range: TextRange) -> Vec<&SymbolRecord> {
        self.records_for_indices(self.by_range.get(&range))
    }

    pub fn find_by_kind(&self, kind: SymbolKindName) -> Vec<&SymbolRecord> {
        self.records_for_indices(self.by_kind.get(&kind))
    }

    pub fn find_by_file_position(
        &self,
        file: &str,
        line: u32,
        character: u32,
    ) -> Option<&SymbolRecord> {
        self.symbols
            .iter()
            .filter(|symbol| {
                symbol.file == file && contains_position(symbol.range, line, character)
            })
            .min_by_key(|symbol| range_span(symbol.range))
    }

    pub fn find_by_uri_path_position(
        &self,
        uri_path: &Path,
        line: u32,
        character: u32,
    ) -> Option<&SymbolRecord> {
        self.symbols
            .iter()
            .filter(|symbol| {
                Path::new(&symbol.file) == uri_path
                    || uri_path.ends_with(&symbol.file)
                    || Path::new(&symbol.file).ends_with(uri_path)
            })
            .filter(|symbol| contains_position(symbol.range, line, character))
            .min_by_key(|symbol| range_span(symbol.range))
    }

    fn records_for_indices(&self, indices: Option<&Vec<usize>>) -> Vec<&SymbolRecord> {
        indices
            .into_iter()
            .flat_map(|indices| indices.iter())
            .filter_map(|idx| self.symbols.get(*idx))
            .collect()
    }
}

impl SymbolRecord {
    pub fn from_node(node: &GraphNode) -> Option<Self> {
        Some(Self {
            id: node.id.clone(),
            node_id: node.id.clone(),
            language: node
                .language
                .as_deref()
                .map(LanguageId::from)
                .unwrap_or(LanguageId::Rust),
            node_type: node.node_type,
            label: node.label.clone(),
            name: node.label.clone(),
            kind: SymbolKindName::from_node_type(node.node_type),
            file: node.file.clone()?,
            module: node.module.clone(),
            crate_name: node.crate_name.clone(),
            line: node.line.unwrap_or(node.selection_range?.start.line + 1),
            character: node.selection_range?.start.character,
            range: node.range?,
            selection_range: node.selection_range?,
        })
    }
}

fn contains_position(range: LspRange, line: u32, character: u32) -> bool {
    let after_start =
        line > range.start.line || (line == range.start.line && character >= range.start.character);
    let before_end =
        line < range.end.line || (line == range.end.line && character <= range.end.character);
    after_start && before_end
}

fn range_span(range: LspRange) -> u32 {
    range.end.line.saturating_sub(range.start.line) * 10_000
        + range.end.character.saturating_sub(range.start.character)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SymbolKindName {
    File,
    Module,
    Struct,
    Enum,
    Trait,
    Function,
    Method,
    Constructor,
    Object,
    Package,
    Namespace,
    Class,
    Macro,
    Impl,
    Component,
    Hook,
    Interface,
    TypeAlias,
    Endpoint,
    ExternalCrate,
    Other,
}

impl SymbolKindName {
    pub fn from_node_type(node_type: NodeType) -> Self {
        match node_type {
            NodeType::File => Self::File,
            NodeType::Module => Self::Module,
            NodeType::Struct => Self::Struct,
            NodeType::Class => Self::Class,
            NodeType::Enum => Self::Enum,
            NodeType::Trait => Self::Trait,
            NodeType::Function => Self::Function,
            NodeType::Method => Self::Method,
            NodeType::Macro => Self::Macro,
            NodeType::Impl => Self::Impl,
            NodeType::Component => Self::Component,
            NodeType::Hook => Self::Hook,
            NodeType::Interface => Self::Interface,
            NodeType::TypeAlias => Self::TypeAlias,
            NodeType::Endpoint => Self::Endpoint,
            NodeType::ExternalCrate => Self::ExternalCrate,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: SymbolKindName,
    pub file: Option<String>,
    pub line: u32,
    pub range: Option<LspRange>,
    pub selection_range: Option<LspRange>,
    pub children: Vec<DiscoveredSymbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(rename = "crate", skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_async: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_unsafe: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_generic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmarked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<LspRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_range: Option<LspRange>,
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Visibility {
    #[serde(rename = "pub")]
    Pub,
    #[serde(rename = "pub(crate)")]
    PubCrate,
    #[serde(rename = "private")]
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    #[serde(default)]
    pub confidence: EdgeConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub module: String,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub functions_count: u32,
    pub links_count: u32,
    pub diagnostics_count: u32,
    pub complexity: Complexity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Complexity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: AnalysisEventType,
    pub message: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisEventType {
    Info,
    Warning,
    Error,
    Analyzer,
    Graph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub files: Vec<ProjectFile>,
    pub events: Vec<AnalysisEvent>,
    pub status: AppStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPatch {
    pub added_nodes: Vec<GraphNode>,
    pub updated_nodes: Vec<GraphNode>,
    pub removed_node_ids: Vec<String>,
    pub added_edges: Vec<GraphEdge>,
    pub updated_edges: Vec<GraphEdge>,
    pub removed_edge_ids: Vec<String>,
    pub diagnostics: Vec<DiagnosticRecord>,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub app_state: AppState,
    pub analyzer_status: AnalyzerStatus,
    pub project_name: Option<String>,
    pub project_path: Option<String>,
    pub last_updated: Option<String>,
    pub message: Option<String>,
    pub progress: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub file: Option<String>,
    pub module: Option<String>,
    #[serde(rename = "crate")]
    pub crate_name: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusRequest {
    pub node_id: String,
    pub depth: FocusDepth,
    pub mode: GraphMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FocusDepth {
    Number(u8),
    Full(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusResponse {
    pub center: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetailsResponse {
    pub node: GraphNode,
    pub incoming_edges: Vec<GraphEdge>,
    pub outgoing_edges: Vec<GraphEdge>,
    pub callers: Vec<GraphNode>,
    pub callees: Vec<GraphNode>,
    pub references: Vec<ReferenceRecord>,
    pub related_types: Vec<GraphNode>,
    pub diagnostics: Vec<DiagnosticRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<GraphNode>,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    pub character: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<TextRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
#[serde(rename_all = "snake_case")]
pub enum ServerMessage {
    GraphSnapshot(GraphSnapshot),
    GraphPatch(GraphPatch),
    AnalyzerStatus(AppStatus),
    AnalysisEvent(AnalysisEvent),
    Error { message: String },
}

impl AppStatus {
    pub fn empty() -> Self {
        Self {
            app_state: AppState::Empty,
            analyzer_status: AnalyzerStatus::Starting,
            project_name: None,
            project_path: None,
            last_updated: None,
            message: None,
            progress: None,
        }
    }
}

pub fn edge_id(edge_type: EdgeType, source: &str, target: &str) -> String {
    format!("{edge_type:?}:{source}->{target}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(line: u32, start: u32, end: u32) -> TextRange {
        TextRange {
            start: TextPosition {
                line,
                character: start,
            },
            end: TextPosition {
                line,
                character: end,
            },
        }
    }

    fn symbol(
        id: &str,
        language: LanguageId,
        file: &str,
        name: &str,
        kind: SymbolKindName,
        range: TextRange,
    ) -> SymbolRecord {
        let name = name.to_string();
        SymbolRecord {
            id: id.into(),
            node_id: id.into(),
            language,
            node_type: NodeType::Function,
            label: name.clone(),
            name,
            kind,
            file: file.into(),
            module: Some("test".into()),
            crate_name: Some("test".into()),
            line: range.start.line + 1,
            character: range.start.character,
            range,
            selection_range: range,
        }
    }

    #[test]
    fn symbol_index_stores_rust_and_typescript_together() {
        let rust_range = range(2, 0, 12);
        let ts_range = range(4, 7, 21);
        let index = SymbolIndex::new(vec![
            symbol(
                "fn:demo::main@3",
                LanguageId::Rust,
                "src/main.rs",
                "main",
                SymbolKindName::Function,
                rust_range,
            ),
            symbol(
                "component:frontend/src/App.tsx::App@5",
                LanguageId::TypeScript,
                "frontend/src/App.tsx",
                "App",
                SymbolKindName::Component,
                ts_range,
            ),
        ]);

        assert_eq!(index.get("fn:demo::main@3").unwrap().name, "main");
        assert_eq!(
            index.find_by_node_id("fn:demo::main@3").unwrap().label,
            "main"
        );
        assert_eq!(index.find_by_language(&LanguageId::Rust).len(), 1);
        assert_eq!(index.find_by_language(&LanguageId::TypeScript).len(), 1);
        assert_eq!(index.find_by_file("frontend/src/App.tsx")[0].name, "App");
        assert_eq!(index.find_by_name("main")[0].language, LanguageId::Rust);
        assert_eq!(index.find_by_range(ts_range)[0].name, "App");
        assert_eq!(
            index.find_by_kind(SymbolKindName::Component)[0].language,
            LanguageId::TypeScript
        );
    }
}
