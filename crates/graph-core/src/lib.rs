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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
