use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    Qml,
    Other(String),
}

impl LanguageId {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Qml => "qml",
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
            "qml" => Self::Qml,
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
    Object,
    Enum,
    Trait,
    Impl,
    Function,
    Method,
    Component,
    Hook,
    Interface,
    TypeAlias,
    Property,
    Signal,
    Handler,
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
pub enum DataFlowKind {
    Argument,
    ReturnValue,
    Assignment,
    StateUpdate,
    PropertyBinding,
    ApiRequest,
    ApiResponse,
    ModelUse,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceReachability {
    Active,
    Detached,
    Generated,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraceKind {
    Route,
    DataFlow,
    NodeNeighborhood,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TraceStepKind {
    Caller,
    ApiRequest,
    Endpoint,
    EndpointHandler,
    BackendHandler,
    ServiceCall,
    ModelUse,
    ReturnValue,
    ApiResponse,
    StateUpdate,
    PropertyBinding,
    DetachedSource,
    ExternalDependency,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceStep {
    pub id: String,
    pub kind: TraceStepKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<EdgeConfidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reachability: Option<SourceReachability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceExplanation {
    pub id: String,
    pub kind: TraceKind,
    pub title: String,
    pub summary: String,
    pub steps: Vec<TraceStep>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_key: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextPackKind {
    Node,
    Trace,
    Route,
    DataFlow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSnippet {
    pub id: String,
    pub file: String,
    pub language: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub code: String,
    pub related_node_ids: Vec<String>,
    pub related_edge_ids: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPack {
    pub id: String,
    pub kind: ContextPackKind,
    pub title: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub snippets: Vec<ContextSnippet>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub diagnostics: Vec<DiagnosticRecord>,
    pub warnings: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteKey {
    pub method: String,
    pub path: String,
    pub key: String,
}

pub fn route_key(method: &str, path: &str) -> RouteKey {
    let method = method.trim().to_ascii_uppercase();
    let normalized_path = normalize_route_path(path);
    RouteKey {
        key: format!("{method} {normalized_path}"),
        method,
        path: normalized_path,
    }
}

pub fn route_key_from_label(label: &str) -> Option<RouteKey> {
    let (method, path) = label.split_once(char::is_whitespace)?;
    Some(route_key(method, path.trim()))
}

fn normalize_route_path(path: &str) -> String {
    let path = path.trim();
    if path == "/" {
        return "/".to_string();
    }
    let mut path = path.trim_end_matches('/').to_string();
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    path
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
    Property,
    Signal,
    Handler,
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
            NodeType::Object => Self::Object,
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
            NodeType::Property => Self::Property,
            NodeType::Signal => Self::Signal,
            NodeType::Handler => Self::Handler,
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

/// Current version of the canonical graph JSON contract.
pub const GRAPH_SCHEMA_VERSION: u32 = 1;
const LEGACY_GRAPH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reachability: Option<SourceReachability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reachable_from: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached_reason: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    #[serde(default)]
    pub confidence: EdgeConfidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(alias = "data_flow_kind", skip_serializing_if = "Option::is_none")]
    pub data_flow_kind: Option<DataFlowKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
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

/// Backward-compatible API envelope consumed by the existing frontend and MCP adapters.
///
/// New persistence and hashing code should convert this envelope to [`VersionedGraphSnapshot`]
/// and hash only [`CanonicalGraphSnapshot::canonical_json_bytes`].
#[derive(Debug, Clone)]
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
    #[serde(default)]
    pub analyzers: Vec<AnalyzerServiceStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python_analyzer: Option<PythonAnalyzerStatus>,
    pub project_name: Option<String>,
    pub project_path: Option<String>,
    pub last_updated: Option<String>,
    pub message: Option<String>,
    pub progress: Option<u8>,
}

/// Source-derived node fields that participate in canonical graph serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalGraphNode {
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
    pub range: Option<LspRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_range: Option<LspRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reachability: Option<SourceReachability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reachable_from: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached_reason: Option<String>,
}

/// Source-derived edge fields that participate in canonical graph serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalGraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    #[serde(default)]
    pub confidence: EdgeConfidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(alias = "data_flow_kind", skip_serializing_if = "Option::is_none")]
    pub data_flow_kind: Option<DataFlowKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// Deterministic, versioned graph data. Runtime, analysis, and layout fields are excluded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalGraphSnapshot {
    pub graph_schema_version: u32,
    pub nodes: Vec<CanonicalGraphNode>,
    pub edges: Vec<CanonicalGraphEdge>,
}

/// Derived analysis information that is useful to clients but is not part of the canonical hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphAnalysisMetadata {
    #[serde(default)]
    pub files: Vec<ProjectFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_metrics: Vec<NodeAnalysisMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeAnalysisMetadata {
    pub node_id: String,
    pub connections: u32,
}

/// Volatile analysis status and event history. Timestamps and progress live only here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRuntimeStatus {
    pub status: AppStatus,
    #[serde(default)]
    pub events: Vec<AnalysisEvent>,
}

/// UI-owned layout and per-user state, keyed by canonical node ID.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphLayoutArtifact {
    #[serde(default)]
    pub nodes: Vec<NodeLayoutArtifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeLayoutArtifact {
    pub node_id: String,
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmarked: Option<bool>,
}

/// New separated representation. The canonical graph remains at the top level while non-canonical
/// concerns are explicit sibling objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionedGraphSnapshot {
    #[serde(flatten)]
    pub graph: CanonicalGraphSnapshot,
    pub analysis_metadata: GraphAnalysisMetadata,
    pub runtime_status: GraphRuntimeStatus,
    pub layout: GraphLayoutArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalGraphError {
    UnsupportedSchemaVersion {
        found: u32,
        supported: u32,
    },
    DuplicateNodeId(String),
    DuplicateEdgeId(String),
    DanglingEdge {
        edge_id: String,
        endpoint_id: String,
    },
    InvalidSourceRange {
        node_id: String,
        field: &'static str,
    },
    DuplicateLayoutNodeId(String),
    UnknownLayoutNodeId(String),
    DuplicateNodeMetricId(String),
    UnknownNodeMetricId(String),
    Serialization(String),
}

impl fmt::Display for CanonicalGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                formatter,
                "unsupported graph schema version {found}; supported version is {supported}"
            ),
            Self::DuplicateNodeId(id) => write!(formatter, "duplicate canonical node ID: {id}"),
            Self::DuplicateEdgeId(id) => write!(formatter, "duplicate canonical edge ID: {id}"),
            Self::DanglingEdge {
                edge_id,
                endpoint_id,
            } => write!(
                formatter,
                "canonical edge {edge_id} references missing node {endpoint_id}"
            ),
            Self::InvalidSourceRange { node_id, field } => {
                write!(formatter, "canonical node {node_id} has invalid {field}")
            }
            Self::DuplicateLayoutNodeId(id) => {
                write!(formatter, "duplicate layout artifact for node {id}")
            }
            Self::UnknownLayoutNodeId(id) => {
                write!(formatter, "layout artifact references unknown node {id}")
            }
            Self::DuplicateNodeMetricId(id) => {
                write!(formatter, "duplicate analysis metadata for node {id}")
            }
            Self::UnknownNodeMetricId(id) => {
                write!(formatter, "analysis metadata references unknown node {id}")
            }
            Self::Serialization(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for CanonicalGraphError {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GraphSnapshotRef<'a> {
    graph_schema_version: u32,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    files: &'a [ProjectFile],
    events: &'a [AnalysisEvent],
    status: &'a AppStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphSnapshotWire {
    #[serde(
        default = "legacy_graph_schema_version",
        alias = "graph_schema_version"
    )]
    graph_schema_version: u32,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    files: Vec<ProjectFile>,
    events: Vec<AnalysisEvent>,
    status: AppStatus,
}

fn legacy_graph_schema_version() -> u32 {
    LEGACY_GRAPH_SCHEMA_VERSION
}

impl Serialize for GraphSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        GraphSnapshotRef {
            graph_schema_version: GRAPH_SCHEMA_VERSION,
            nodes: &self.nodes,
            edges: &self.edges,
            files: &self.files,
            events: &self.events,
            status: &self.status,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GraphSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = GraphSnapshotWire::deserialize(deserializer)?;
        if wire.graph_schema_version != GRAPH_SCHEMA_VERSION {
            return Err(<D::Error as serde::de::Error>::custom(
                CanonicalGraphError::UnsupportedSchemaVersion {
                    found: wire.graph_schema_version,
                    supported: GRAPH_SCHEMA_VERSION,
                },
            ));
        }
        Ok(Self {
            nodes: wire.nodes,
            edges: wire.edges,
            files: wire.files,
            events: wire.events,
            status: wire.status,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalGraphSnapshotRef<'a> {
    graph_schema_version: u32,
    nodes: &'a [CanonicalGraphNode],
    edges: &'a [CanonicalGraphEdge],
}

impl Serialize for CanonicalGraphSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.validate().map_err(serde::ser::Error::custom)?;
        let normalized = self.normalized();
        CanonicalGraphSnapshotRef {
            graph_schema_version: normalized.graph_schema_version,
            nodes: &normalized.nodes,
            edges: &normalized.edges,
        }
        .serialize(serializer)
    }
}

impl CanonicalGraphSnapshot {
    pub fn validate(&self) -> Result<(), CanonicalGraphError> {
        if self.graph_schema_version != GRAPH_SCHEMA_VERSION {
            return Err(CanonicalGraphError::UnsupportedSchemaVersion {
                found: self.graph_schema_version,
                supported: GRAPH_SCHEMA_VERSION,
            });
        }

        let mut node_ids = HashSet::with_capacity(self.nodes.len());
        for node in &self.nodes {
            if !node_ids.insert(node.id.as_str()) {
                return Err(CanonicalGraphError::DuplicateNodeId(node.id.clone()));
            }
            validate_source_range(node, "range", node.range)?;
            validate_source_range(node, "selectionRange", node.selection_range)?;
        }

        let mut edge_ids = HashSet::with_capacity(self.edges.len());
        for edge in &self.edges {
            if !edge_ids.insert(edge.id.as_str()) {
                return Err(CanonicalGraphError::DuplicateEdgeId(edge.id.clone()));
            }
            for endpoint_id in [&edge.source, &edge.target] {
                if !node_ids.contains(endpoint_id.as_str()) {
                    return Err(CanonicalGraphError::DanglingEdge {
                        edge_id: edge.id.clone(),
                        endpoint_id: endpoint_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Returns the deterministic JSON bytes that define the canonical graph hash input.
    pub fn canonical_json_bytes(&self) -> Result<Vec<u8>, CanonicalGraphError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|error| {
            CanonicalGraphError::Serialization(format!(
                "failed to serialize canonical graph: {error}"
            ))
        })
    }

    pub fn normalized(&self) -> Self {
        let mut normalized = self.clone();
        for node in &mut normalized.nodes {
            normalize_reachable_from(&mut node.reachable_from);
        }
        normalized.nodes.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.label.cmp(&right.label))
        });
        normalized.edges.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.source.cmp(&right.source))
                .then_with(|| left.target.cmp(&right.target))
        });
        normalized
    }
}

impl GraphSnapshot {
    pub fn canonical_graph(&self) -> CanonicalGraphSnapshot {
        CanonicalGraphSnapshot {
            graph_schema_version: GRAPH_SCHEMA_VERSION,
            nodes: self.nodes.iter().map(CanonicalGraphNode::from).collect(),
            edges: self.edges.iter().map(CanonicalGraphEdge::from).collect(),
        }
    }

    pub fn versioned(&self) -> VersionedGraphSnapshot {
        VersionedGraphSnapshot::from(self)
    }
}

impl From<&GraphNode> for CanonicalGraphNode {
    fn from(node: &GraphNode) -> Self {
        Self {
            id: node.id.clone(),
            language: node.language.clone(),
            node_type: node.node_type,
            label: node.label.clone(),
            file: node.file.clone(),
            module: node.module.clone(),
            crate_name: node.crate_name.clone(),
            line: node.line,
            visibility: node.visibility,
            is_async: node.is_async,
            is_unsafe: node.is_unsafe,
            is_generic: node.is_generic,
            signature: node.signature.clone(),
            description: node.description.clone(),
            range: node.range,
            selection_range: node.selection_range,
            reachability: node.reachability,
            reachable_from: node.reachable_from.clone(),
            detached_reason: node.detached_reason.clone(),
        }
    }
}

impl From<&GraphEdge> for CanonicalGraphEdge {
    fn from(edge: &GraphEdge) -> Self {
        Self {
            id: edge.id.clone(),
            source: edge.source.clone(),
            target: edge.target.clone(),
            edge_type: edge.edge_type,
            confidence: edge.confidence,
            label: edge.label.clone(),
            description: edge.description.clone(),
            data_flow_kind: edge.data_flow_kind,
            evidence: edge.evidence.clone(),
        }
    }
}

impl From<&GraphSnapshot> for VersionedGraphSnapshot {
    fn from(snapshot: &GraphSnapshot) -> Self {
        let mut node_metrics = snapshot
            .nodes
            .iter()
            .filter_map(|node| {
                node.connections.map(|connections| NodeAnalysisMetadata {
                    node_id: node.id.clone(),
                    connections,
                })
            })
            .collect::<Vec<_>>();
        node_metrics.sort_by(|left, right| left.node_id.cmp(&right.node_id));

        let mut layout_nodes = snapshot
            .nodes
            .iter()
            .map(|node| NodeLayoutArtifact {
                node_id: node.id.clone(),
                x: node.x,
                y: node.y,
                vx: node.vx,
                vy: node.vy,
                pinned: node.pinned,
                bookmarked: node.bookmarked,
            })
            .collect::<Vec<_>>();
        layout_nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));

        Self {
            graph: snapshot.canonical_graph(),
            analysis_metadata: GraphAnalysisMetadata {
                files: snapshot.files.clone(),
                node_metrics,
                node_order: snapshot.nodes.iter().map(|node| node.id.clone()).collect(),
                edge_order: snapshot.edges.iter().map(|edge| edge.id.clone()).collect(),
            },
            runtime_status: GraphRuntimeStatus {
                status: snapshot.status.clone(),
                events: snapshot.events.clone(),
            },
            layout: GraphLayoutArtifact {
                nodes: layout_nodes,
            },
        }
    }
}

impl From<GraphSnapshot> for VersionedGraphSnapshot {
    fn from(snapshot: GraphSnapshot) -> Self {
        Self::from(&snapshot)
    }
}

impl TryFrom<VersionedGraphSnapshot> for GraphSnapshot {
    type Error = CanonicalGraphError;

    fn try_from(snapshot: VersionedGraphSnapshot) -> Result<Self, Self::Error> {
        snapshot.graph.validate()?;
        let canonical_node_ids = snapshot
            .graph
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<HashSet<_>>();

        let mut metrics = HashMap::new();
        for metric in snapshot.analysis_metadata.node_metrics {
            if !canonical_node_ids.contains(metric.node_id.as_str()) {
                return Err(CanonicalGraphError::UnknownNodeMetricId(metric.node_id));
            }
            if metrics
                .insert(metric.node_id.clone(), metric.connections)
                .is_some()
            {
                return Err(CanonicalGraphError::DuplicateNodeMetricId(metric.node_id));
            }
        }

        let mut layouts = HashMap::new();
        for layout in snapshot.layout.nodes {
            if !canonical_node_ids.contains(layout.node_id.as_str()) {
                return Err(CanonicalGraphError::UnknownLayoutNodeId(layout.node_id));
            }
            let node_id = layout.node_id.clone();
            if layouts.insert(node_id.clone(), layout).is_some() {
                return Err(CanonicalGraphError::DuplicateLayoutNodeId(node_id));
            }
        }

        let nodes = snapshot
            .graph
            .nodes
            .into_iter()
            .map(|node| {
                let connections = metrics.remove(&node.id);
                let layout = layouts.remove(&node.id).unwrap_or(NodeLayoutArtifact {
                    node_id: node.id.clone(),
                    x: 0.0,
                    y: 0.0,
                    vx: 0.0,
                    vy: 0.0,
                    pinned: None,
                    bookmarked: None,
                });
                node.into_legacy(connections, layout)
            })
            .collect::<Vec<_>>();
        let edges = snapshot
            .graph
            .edges
            .into_iter()
            .map(CanonicalGraphEdge::into_legacy)
            .collect::<Vec<_>>();

        Ok(Self {
            nodes: reorder_by_id(nodes, &snapshot.analysis_metadata.node_order, |node| {
                &node.id
            }),
            edges: reorder_by_id(edges, &snapshot.analysis_metadata.edge_order, |edge| {
                &edge.id
            }),
            files: snapshot.analysis_metadata.files,
            events: snapshot.runtime_status.events,
            status: snapshot.runtime_status.status,
        })
    }
}

impl VersionedGraphSnapshot {
    pub fn into_legacy(self) -> Result<GraphSnapshot, CanonicalGraphError> {
        self.try_into()
    }
}

impl CanonicalGraphNode {
    fn into_legacy(self, connections: Option<u32>, layout: NodeLayoutArtifact) -> GraphNode {
        GraphNode {
            id: self.id,
            language: self.language,
            node_type: self.node_type,
            label: self.label,
            file: self.file,
            module: self.module,
            crate_name: self.crate_name,
            line: self.line,
            visibility: self.visibility,
            is_async: self.is_async,
            is_unsafe: self.is_unsafe,
            is_generic: self.is_generic,
            signature: self.signature,
            description: self.description,
            pinned: layout.pinned,
            bookmarked: layout.bookmarked,
            connections,
            range: self.range,
            selection_range: self.selection_range,
            reachability: self.reachability,
            reachable_from: self.reachable_from,
            detached_reason: self.detached_reason,
            x: layout.x,
            y: layout.y,
            vx: layout.vx,
            vy: layout.vy,
        }
    }
}

impl CanonicalGraphEdge {
    fn into_legacy(self) -> GraphEdge {
        GraphEdge {
            id: self.id,
            source: self.source,
            target: self.target,
            edge_type: self.edge_type,
            confidence: self.confidence,
            label: self.label,
            description: self.description,
            data_flow_kind: self.data_flow_kind,
            evidence: self.evidence,
        }
    }
}

fn normalize_reachable_from(reachable_from: &mut Option<Vec<String>>) {
    if let Some(ids) = reachable_from {
        ids.sort();
        ids.dedup();
    }
}

fn validate_source_range(
    node: &CanonicalGraphNode,
    field: &'static str,
    range: Option<TextRange>,
) -> Result<(), CanonicalGraphError> {
    if range.is_some_and(|range| {
        (range.start.line, range.start.character) > (range.end.line, range.end.character)
    }) {
        return Err(CanonicalGraphError::InvalidSourceRange {
            node_id: node.id.clone(),
            field,
        });
    }
    Ok(())
}

fn reorder_by_id<T, F>(items: Vec<T>, order: &[String], id: F) -> Vec<T>
where
    F: Fn(&T) -> &str,
{
    let mut by_id = items
        .into_iter()
        .map(|item| (id(&item).to_string(), item))
        .collect::<HashMap<_, _>>();
    let mut ordered = Vec::with_capacity(by_id.len());
    for item_id in order {
        if let Some(item) = by_id.remove(item_id) {
            ordered.push(item);
        }
    }
    let mut remaining = by_id.into_values().collect::<Vec<_>>();
    remaining.sort_by(|left, right| id(left).cmp(id(right)));
    ordered.extend(remaining);
    ordered
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnalyzerKind {
    Rust,
    TypeScript,
    Python,
    Qml,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnalyzerEngine {
    RustAnalyzer,
    Ty,
    TypeScriptParser,
    TypeScriptLanguageServer,
    QmlParser,
    QmlLanguageServer,
    TreeSitter,
    Parser,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnalyzerCapability {
    Symbols,
    Diagnostics,
    References,
    Definitions,
    TypeDefinitions,
    CallHierarchy,
    SemanticCalls,
    SemanticTokens,
    Formatting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalyzerProvider {
    Local,
    Cloud,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzerServiceStatus {
    pub id: String,
    pub kind: AnalyzerKind,
    pub engine: AnalyzerEngine,
    pub label: String,
    pub status: AnalyzerStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub capabilities: Vec<AnalyzerCapability>,
    pub files_indexed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
    pub provider: AnalyzerProvider,
    #[serde(default)]
    pub billable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_used: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnalysisJobStatus {
    Queued,
    Preparing,
    Indexing,
    RunningAnalyzers,
    BuildingGraph,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnalysisMode {
    Full,
    Incremental,
    FallbackFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnalysisJobSourceKind {
    LocalPath,
    UploadedArchive,
    GitRepository,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJobSource {
    pub kind: AnalysisJobSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJob {
    pub id: String,
    pub status: AnalysisJobStatus,
    pub source: AnalysisJobSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    #[serde(default = "default_analysis_mode")]
    pub analysis_mode: AnalysisMode,
    pub requested_analyzers: Vec<AnalyzerEngine>,
    pub analyzer_statuses: Vec<AnalyzerServiceStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_estimated: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits_used: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn default_analysis_mode() -> AnalysisMode {
    AnalysisMode::Full
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAnalysisJobRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AnalysisJobSource>,
    #[serde(default)]
    pub requested_analyzers: Vec<AnalyzerEngine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    #[serde(default)]
    pub incremental: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_revision_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAnalysisUsage {
    pub job_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<String>,
    pub input_files: u32,
    pub input_bytes: u64,
    pub output_nodes: u32,
    pub output_edges: u32,
    pub output_files: u32,
    pub requested_analyzers: Vec<AnalyzerEngine>,
    pub materialization_ms: u64,
    pub graph_build_ms: u64,
    pub total_wall_ms: u64,
    pub credits_estimated: u32,
    pub credits_used: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

pub fn estimate_cloud_analysis_credits(
    input_files: u32,
    input_bytes: u64,
    requested_analyzers: &[AnalyzerEngine],
) -> u32 {
    let base = 1u32;
    let file_units = input_files.saturating_add(99) / 100;
    let byte_units = ((input_bytes.saturating_add(10 * 1024 * 1024 - 1)) / (10 * 1024 * 1024))
        .min(u32::MAX as u64) as u32;
    let analyzer_units = if requested_analyzers.is_empty() {
        1
    } else {
        requested_analyzers.iter().fold(0u32, |units, analyzer| {
            units
                + match analyzer {
                    AnalyzerEngine::RustAnalyzer => 4,
                    AnalyzerEngine::Ty
                    | AnalyzerEngine::TypeScriptLanguageServer
                    | AnalyzerEngine::QmlLanguageServer => 2,
                    _ => 1,
                }
        })
    };

    base.saturating_add(file_units)
        .saturating_add(byte_units)
        .saturating_add(analyzer_units)
        .max(1)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudWorkspace {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AnalysisJobSource>,
    pub current_revision: Option<String>,
    pub files_count: u32,
    pub total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFileEntry {
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRevision {
    pub id: String,
    pub workspace_id: String,
    pub files: Vec<WorkspaceFileEntry>,
    pub files_count: u32,
    pub total_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSyncPlanRequest {
    pub base_revision: Option<String>,
    pub files: Vec<WorkspaceFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSyncPlanResponse {
    pub missing_hashes: Vec<String>,
    pub known_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspaceRevisionRequest {
    pub base_revision: Option<String>,
    pub files: Vec<WorkspaceFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspaceRevisionResponse {
    pub workspace: CloudWorkspace,
    pub revision: WorkspaceRevision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonAnalyzerStatus {
    pub mode: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_details: Option<EndpointDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDetails {
    pub route_method: String,
    pub route_path: String,
    pub route_key: String,
    pub endpoint_language: Option<String>,
    pub handlers: Vec<EndpointHandlerDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointHandlerDetails {
    pub node_id: String,
    pub label: String,
    pub handler_language: Option<String>,
    pub handler_file: Option<String>,
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
            analyzers: Vec::new(),
            python_analyzer: None,
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

    fn graph_node(id: &str, label: &str, x: f64) -> GraphNode {
        GraphNode {
            id: id.into(),
            language: Some("rust".into()),
            node_type: NodeType::Function,
            label: label.into(),
            file: Some(format!("src/{label}.rs")),
            module: Some("demo".into()),
            crate_name: Some("demo".into()),
            line: Some(1),
            visibility: Some(Visibility::Pub),
            is_async: Some(false),
            is_unsafe: Some(false),
            is_generic: Some(false),
            signature: Some(format!("fn {label}()")),
            description: None,
            pinned: Some(true),
            bookmarked: Some(false),
            connections: Some(1),
            range: Some(range(0, 0, 5)),
            selection_range: Some(range(0, 3, 5)),
            reachability: Some(SourceReachability::Active),
            reachable_from: Some(vec!["root-b".into(), "root-a".into(), "root-a".into()]),
            detached_reason: None,
            x,
            y: x + 1.0,
            vx: x + 2.0,
            vy: x + 3.0,
        }
    }

    fn graph_snapshot() -> GraphSnapshot {
        GraphSnapshot {
            nodes: vec![
                graph_node("node-b", "beta", 20.0),
                graph_node("node-a", "alpha", 10.0),
            ],
            edges: vec![GraphEdge {
                id: "edge-b-a".into(),
                source: "node-b".into(),
                target: "node-a".into(),
                edge_type: EdgeType::Calls,
                confidence: EdgeConfidence::Exact,
                label: Some("calls".into()),
                description: None,
                data_flow_kind: None,
                evidence: Some("src/beta.rs:1".into()),
            }],
            files: vec![ProjectFile {
                id: "src/beta.rs".into(),
                name: "beta.rs".into(),
                path: "src/beta.rs".into(),
                module: "demo".into(),
                crate_name: "demo".into(),
                functions_count: 1,
                links_count: 1,
                diagnostics_count: 0,
                complexity: Complexity::Low,
            }],
            events: vec![AnalysisEvent {
                id: "event-1".into(),
                event_type: AnalysisEventType::Graph,
                message: "complete".into(),
                timestamp: "2026-07-21T10:00:00Z".into(),
                file: None,
            }],
            status: AppStatus {
                app_state: AppState::Normal,
                analyzer_status: AnalyzerStatus::Ready,
                analyzers: Vec::new(),
                python_analyzer: None,
                project_name: Some("demo".into()),
                project_path: Some("/tmp/demo".into()),
                last_updated: Some("2026-07-21T10:00:01Z".into()),
                message: Some("ready".into()),
                progress: Some(100),
            },
        }
    }

    #[test]
    fn legacy_graph_snapshot_envelope_is_versioned_and_flat() {
        let value = serde_json::to_value(graph_snapshot()).expect("serialize legacy snapshot");

        assert_eq!(value["graphSchemaVersion"], GRAPH_SCHEMA_VERSION);
        assert!(value.get("nodes").is_some());
        assert!(value.get("edges").is_some());
        assert!(value.get("status").is_some());
        assert_eq!(value["nodes"][0]["x"], 20.0);
        assert!(value.get("analysisMetadata").is_none());
    }

    #[test]
    fn legacy_graph_snapshot_without_version_remains_readable() {
        let mut value = serde_json::to_value(graph_snapshot()).expect("serialize legacy snapshot");
        value
            .as_object_mut()
            .expect("snapshot object")
            .remove("graphSchemaVersion");

        let decoded: GraphSnapshot =
            serde_json::from_value(value).expect("read pre-version snapshot");

        assert_eq!(decoded.nodes.len(), 2);
        assert_eq!(decoded.status.progress, Some(100));
    }

    #[test]
    fn legacy_graph_snapshot_rejects_unknown_schema_version() {
        let mut value = serde_json::to_value(graph_snapshot()).expect("serialize legacy snapshot");
        value["graphSchemaVersion"] = serde_json::json!(GRAPH_SCHEMA_VERSION + 1);

        let error = serde_json::from_value::<GraphSnapshot>(value)
            .expect_err("unknown schema version must fail");

        assert!(error
            .to_string()
            .contains("unsupported graph schema version"));
    }

    #[test]
    fn canonical_serialization_excludes_analysis_runtime_and_layout() {
        let canonical = graph_snapshot().canonical_graph();
        let value: serde_json::Value = serde_json::from_slice(
            &canonical
                .canonical_json_bytes()
                .expect("serialize canonical graph"),
        )
        .expect("read canonical JSON");

        assert_eq!(value["graphSchemaVersion"], GRAPH_SCHEMA_VERSION);
        assert_eq!(value["nodes"][0]["id"], "node-a");
        assert_eq!(
            value["nodes"][0]["reachableFrom"],
            serde_json::json!(["root-a", "root-b"])
        );
        for excluded in [
            "x",
            "y",
            "vx",
            "vy",
            "pinned",
            "bookmarked",
            "connections",
            "timestamp",
            "progress",
            "status",
            "files",
            "events",
        ] {
            assert!(
                !value.to_string().contains(&format!("\"{excluded}\":")),
                "canonical JSON contains excluded field {excluded}"
            );
        }
    }

    #[test]
    fn canonical_bytes_ignore_input_order_and_volatile_fields() {
        let first = graph_snapshot();
        let mut second = first.clone();
        second.nodes.reverse();
        second.edges.reverse();
        second.nodes[0].reachable_from = Some(vec!["root-b".into(), "root-a".into()]);
        second.nodes[0].x = 9_999.0;
        second.nodes[0].bookmarked = Some(true);
        second.status.progress = Some(7);
        second.status.last_updated = Some("later".into());
        second.events[0].timestamp = "later".into();

        assert_eq!(
            first
                .canonical_graph()
                .canonical_json_bytes()
                .expect("first canonical graph"),
            second
                .canonical_graph()
                .canonical_json_bytes()
                .expect("second canonical graph")
        );
    }

    #[test]
    fn separated_snapshot_round_trips_the_legacy_envelope() {
        let legacy = graph_snapshot();
        let expected = serde_json::to_value(&legacy).expect("serialize expected snapshot");
        let versioned = legacy.versioned();
        let separated = serde_json::to_value(&versioned).expect("serialize separated snapshot");

        assert!(separated.get("analysisMetadata").is_some());
        assert!(separated.get("runtimeStatus").is_some());
        assert!(separated.get("layout").is_some());
        assert!(separated["nodes"][0].get("x").is_none());

        let restored = versioned.into_legacy().expect("restore legacy snapshot");
        assert_eq!(
            serde_json::to_value(restored).expect("serialize restored snapshot"),
            expected
        );
    }

    #[test]
    fn separated_snapshot_json_is_deserializable() {
        let versioned = graph_snapshot().versioned();
        let bytes = serde_json::to_vec(&versioned).expect("serialize separated snapshot");
        let decoded: VersionedGraphSnapshot =
            serde_json::from_slice(&bytes).expect("deserialize separated snapshot");
        let restored = decoded.into_legacy().expect("restore decoded snapshot");

        assert_eq!(restored.nodes[0].id, "node-b");
        assert_eq!(restored.nodes[0].x, 20.0);
        assert_eq!(restored.nodes[0].bookmarked, Some(false));
        assert_eq!(restored.status.progress, Some(100));
        assert_eq!(restored.events[0].timestamp, "2026-07-21T10:00:00Z");
    }

    #[test]
    fn canonical_validation_rejects_dangling_edges() {
        let mut canonical = graph_snapshot().canonical_graph();
        canonical.edges[0].target = "missing".into();

        assert_eq!(
            canonical.validate(),
            Err(CanonicalGraphError::DanglingEdge {
                edge_id: "edge-b-a".into(),
                endpoint_id: "missing".into(),
            })
        );
    }

    #[test]
    fn canonical_validation_rejects_duplicate_ids_ranges_and_versions() {
        let mut duplicate_node = graph_snapshot().canonical_graph();
        duplicate_node.nodes.push(duplicate_node.nodes[0].clone());
        assert!(matches!(
            duplicate_node.validate(),
            Err(CanonicalGraphError::DuplicateNodeId(id)) if id == "node-b"
        ));

        let mut duplicate_edge = graph_snapshot().canonical_graph();
        duplicate_edge.edges.push(duplicate_edge.edges[0].clone());
        assert!(matches!(
            duplicate_edge.validate(),
            Err(CanonicalGraphError::DuplicateEdgeId(id)) if id == "edge-b-a"
        ));

        let mut invalid_range = graph_snapshot().canonical_graph();
        invalid_range.nodes[0].range = Some(TextRange {
            start: TextPosition {
                line: 2,
                character: 0,
            },
            end: TextPosition {
                line: 1,
                character: 0,
            },
        });
        assert!(matches!(
            invalid_range.validate(),
            Err(CanonicalGraphError::InvalidSourceRange {
                node_id,
                field: "range"
            }) if node_id == "node-b"
        ));

        let mut unsupported = graph_snapshot().canonical_graph();
        unsupported.graph_schema_version += 1;
        assert_eq!(
            unsupported.validate(),
            Err(CanonicalGraphError::UnsupportedSchemaVersion {
                found: GRAPH_SCHEMA_VERSION + 1,
                supported: GRAPH_SCHEMA_VERSION,
            })
        );
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

    #[test]
    fn route_keys_normalize_method_and_path() {
        let key = route_key("get", "api/users/");
        assert_eq!(key.method, "GET");
        assert_eq!(key.path, "/api/users");
        assert_eq!(key.key, "GET /api/users");
        assert_eq!(
            route_key_from_label("POST /api/users").unwrap(),
            route_key("post", "/api/users")
        );
    }

    #[test]
    fn analyzer_service_status_serializes_provider_metadata() {
        let status = AnalyzerServiceStatus {
            id: "rust-analyzer".into(),
            kind: AnalyzerKind::Rust,
            engine: AnalyzerEngine::RustAnalyzer,
            label: "rust-analyzer".into(),
            status: AnalyzerStatus::Ready,
            mode: None,
            message: None,
            capabilities: vec![AnalyzerCapability::Symbols],
            files_indexed: 2,
            last_updated: None,
            provider: AnalyzerProvider::Local,
            billable: false,
            credits_used: None,
        };

        let value = serde_json::to_value(status).expect("serialize analyzer status");

        assert_eq!(value["provider"], "local");
        assert_eq!(value["billable"], false);
        assert!(value.get("creditsUsed").is_none());
    }

    #[test]
    fn analysis_job_status_serializes_as_camel_case() {
        let value = serde_json::to_value(AnalysisJobStatus::RunningAnalyzers)
            .expect("serialize job status");

        assert_eq!(value, "runningAnalyzers");
    }

    #[test]
    fn analysis_job_source_kind_serializes_as_camel_case() {
        let value = serde_json::to_value(AnalysisJobSourceKind::UploadedArchive)
            .expect("serialize source kind");

        assert_eq!(value, "uploadedArchive");
    }

    #[test]
    fn analysis_job_serializes_expected_shape() {
        let job = AnalysisJob {
            id: "job_1".into(),
            status: AnalysisJobStatus::Queued,
            source: AnalysisJobSource {
                kind: AnalysisJobSourceKind::LocalPath,
                display_name: None,
                path: Some("/tmp/project".into()),
                repository_url: None,
                git_ref: None,
                commit_sha: None,
            },
            project_name: Some("project".into()),
            message: None,
            progress: Some(0),
            analysis_mode: AnalysisMode::Full,
            requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
            analyzer_statuses: Vec::new(),
            created_at: None,
            started_at: None,
            finished_at: None,
            credits_estimated: None,
            credits_used: None,
            error: None,
        };

        let value = serde_json::to_value(job).expect("serialize analysis job");

        assert_eq!(value["id"], "job_1");
        assert_eq!(value["status"], "queued");
        assert_eq!(value["source"]["kind"], "localPath");
        assert_eq!(value["source"]["path"], "/tmp/project");
        assert_eq!(value["projectName"], "project");
        assert_eq!(value["requestedAnalyzers"][0], "RustAnalyzer");
        assert!(value["analyzerStatuses"].as_array().unwrap().is_empty());
        assert_eq!(value["progress"], 0);
    }

    #[test]
    fn analysis_job_omits_absent_optional_fields() {
        let job = AnalysisJob {
            id: "job_1".into(),
            status: AnalysisJobStatus::Queued,
            source: AnalysisJobSource {
                kind: AnalysisJobSourceKind::LocalPath,
                display_name: None,
                path: Some("/tmp/project".into()),
                repository_url: None,
                git_ref: None,
                commit_sha: None,
            },
            project_name: None,
            message: None,
            progress: None,
            analysis_mode: AnalysisMode::Full,
            requested_analyzers: Vec::new(),
            analyzer_statuses: Vec::new(),
            created_at: None,
            started_at: None,
            finished_at: None,
            credits_estimated: None,
            credits_used: None,
            error: None,
        };

        let value = serde_json::to_value(job).expect("serialize analysis job");

        assert!(value.get("creditsEstimated").is_none());
        assert!(value.get("creditsUsed").is_none());
        assert!(value.get("error").is_none());
        assert!(value.get("finishedAt").is_none());
        assert!(value["source"].get("repositoryUrl").is_none());
    }

    #[test]
    fn create_analysis_job_request_serializes_as_camel_case() {
        let request = CreateAnalysisJobRequest {
            source: None,
            requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
            project_name: Some("demo".into()),
            workspace_id: Some("workspace_1".into()),
            revision_id: Some("revision_1".into()),
            incremental: false,
            base_revision_id: None,
        };

        let value = serde_json::to_value(request).expect("serialize create analysis job request");

        assert!(value.get("source").is_none());
        assert_eq!(value["requestedAnalyzers"][0], "RustAnalyzer");
        assert_eq!(value["projectName"], "demo");
        assert_eq!(value["workspaceId"], "workspace_1");
        assert_eq!(value["revisionId"], "revision_1");
    }

    #[test]
    fn cloud_analysis_usage_serializes_as_camel_case() {
        let usage = CloudAnalysisUsage {
            job_id: "job_1".into(),
            workspace_id: Some("workspace_1".into()),
            revision_id: Some("revision_1".into()),
            input_files: 3,
            input_bytes: 42,
            output_nodes: 5,
            output_edges: 6,
            output_files: 3,
            requested_analyzers: vec![AnalyzerEngine::RustAnalyzer],
            materialization_ms: 7,
            graph_build_ms: 8,
            total_wall_ms: 15,
            credits_estimated: 6,
            credits_used: 6,
            created_at: Some("123".into()),
        };

        let value = serde_json::to_value(usage).expect("serialize cloud usage");

        assert_eq!(value["jobId"], "job_1");
        assert_eq!(value["workspaceId"], "workspace_1");
        assert_eq!(value["revisionId"], "revision_1");
        assert_eq!(value["inputFiles"], 3);
        assert_eq!(value["inputBytes"], 42);
        assert_eq!(value["outputNodes"], 5);
        assert_eq!(value["outputEdges"], 6);
        assert_eq!(value["outputFiles"], 3);
        assert_eq!(value["requestedAnalyzers"][0], "RustAnalyzer");
        assert_eq!(value["materializationMs"], 7);
        assert_eq!(value["graphBuildMs"], 8);
        assert_eq!(value["totalWallMs"], 15);
        assert_eq!(value["creditsEstimated"], 6);
        assert_eq!(value["creditsUsed"], 6);
        assert_eq!(value["createdAt"], "123");
    }

    #[test]
    fn cloud_analysis_usage_omits_absent_optional_fields() {
        let usage = CloudAnalysisUsage {
            job_id: "job_1".into(),
            workspace_id: None,
            revision_id: None,
            input_files: 0,
            input_bytes: 0,
            output_nodes: 0,
            output_edges: 0,
            output_files: 0,
            requested_analyzers: Vec::new(),
            materialization_ms: 0,
            graph_build_ms: 0,
            total_wall_ms: 0,
            credits_estimated: 1,
            credits_used: 1,
            created_at: None,
        };

        let value = serde_json::to_value(usage).expect("serialize cloud usage");

        assert!(value.get("workspaceId").is_none());
        assert!(value.get("revisionId").is_none());
        assert!(value.get("createdAt").is_none());
    }

    #[test]
    fn cloud_analysis_credit_estimator_returns_at_least_one() {
        assert!(estimate_cloud_analysis_credits(0, 0, &[]) >= 1);
    }

    #[test]
    fn cloud_analysis_credit_estimator_increases_for_larger_projects() {
        let small = estimate_cloud_analysis_credits(1, 1, &[]);
        let large = estimate_cloud_analysis_credits(250, 25 * 1024 * 1024, &[]);

        assert!(large > small);
    }

    #[test]
    fn cloud_analysis_credit_estimator_increases_for_rust_analyzer() {
        let parser_only = estimate_cloud_analysis_credits(10, 1024, &[]);
        let rust_analyzer =
            estimate_cloud_analysis_credits(10, 1024, &[AnalyzerEngine::RustAnalyzer]);

        assert!(rust_analyzer > parser_only);
    }

    #[test]
    fn workspace_file_entry_serializes_as_camel_case() {
        let entry = WorkspaceFileEntry {
            path: "src/main.rs".into(),
            content_hash: "sha256:abc".into(),
            size_bytes: 12,
            language: Some(LanguageId::Rust),
        };

        let value = serde_json::to_value(entry).expect("serialize workspace file entry");

        assert_eq!(value["path"], "src/main.rs");
        assert_eq!(value["contentHash"], "sha256:abc");
        assert_eq!(value["sizeBytes"], 12);
        assert_eq!(value["language"], "rust");
    }

    #[test]
    fn workspace_revision_serializes_as_camel_case() {
        let revision = WorkspaceRevision {
            id: "rev_1".into(),
            workspace_id: "workspace_1".into(),
            files: vec![WorkspaceFileEntry {
                path: "src/main.rs".into(),
                content_hash: "sha256:abc".into(),
                size_bytes: 12,
                language: Some(LanguageId::Rust),
            }],
            files_count: 1,
            total_bytes: 12,
            parent_revision: Some("rev_0".into()),
            created_at: None,
        };

        let value = serde_json::to_value(revision).expect("serialize workspace revision");

        assert_eq!(value["workspaceId"], "workspace_1");
        assert_eq!(value["filesCount"], 1);
        assert_eq!(value["totalBytes"], 12);
        assert_eq!(value["parentRevision"], "rev_0");
        assert!(value.get("createdAt").is_none());
    }

    #[test]
    fn workspace_sync_plan_request_and_response_serialize_as_camel_case() {
        let request = WorkspaceSyncPlanRequest {
            base_revision: Some("rev_1".into()),
            files: vec![WorkspaceFileEntry {
                path: "src/lib.rs".into(),
                content_hash: "sha256:def".into(),
                size_bytes: 7,
                language: None,
            }],
        };
        let response = WorkspaceSyncPlanResponse {
            missing_hashes: vec!["sha256:def".into()],
            known_hashes: vec!["sha256:abc".into()],
        };

        let request_value = serde_json::to_value(request).expect("serialize sync plan request");
        let response_value = serde_json::to_value(response).expect("serialize sync plan response");

        assert_eq!(request_value["baseRevision"], "rev_1");
        assert_eq!(request_value["files"][0]["contentHash"], "sha256:def");
        assert_eq!(response_value["missingHashes"][0], "sha256:def");
        assert_eq!(response_value["knownHashes"][0], "sha256:abc");
    }
}
