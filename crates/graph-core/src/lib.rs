use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    File,
    Module,
    Struct,
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
    Uses,
    Calls,
    Renders,
    ApiCall,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolRecord {
    pub id: String,
    pub name: String,
    pub kind: SymbolKindName,
    pub file: String,
    pub range: LspRange,
    pub selection_range: LspRange,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolIndex {
    pub symbols: Vec<SymbolRecord>,
    #[serde(skip)]
    by_id: HashMap<String, usize>,
}

impl SymbolIndex {
    pub fn new(symbols: Vec<SymbolRecord>) -> Self {
        let by_id = symbols
            .iter()
            .enumerate()
            .map(|(idx, symbol)| (symbol.id.clone(), idx))
            .collect();
        Self { symbols, by_id }
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
}

impl SymbolRecord {
    pub fn from_node(node: &GraphNode) -> Option<Self> {
        Some(Self {
            id: node.id.clone(),
            name: node.label.clone(),
            kind: SymbolKindName::from_node_type(node.node_type),
            file: node.file.clone()?,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    Other,
}

impl SymbolKindName {
    pub fn from_node_type(node_type: NodeType) -> Self {
        match node_type {
            NodeType::File => Self::File,
            NodeType::Module => Self::Module,
            NodeType::Struct => Self::Struct,
            NodeType::Enum => Self::Enum,
            NodeType::Trait => Self::Trait,
            NodeType::Function => Self::Function,
            NodeType::Method => Self::Method,
            NodeType::Macro => Self::Macro,
            NodeType::Impl
            | NodeType::Component
            | NodeType::Hook
            | NodeType::Interface
            | NodeType::TypeAlias
            | NodeType::Endpoint
            | NodeType::ExternalCrate => Self::Other,
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
    pub references: Vec<GraphNode>,
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
