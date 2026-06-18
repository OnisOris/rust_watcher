use serde::{Deserialize, Serialize};

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
    Macro,
    ExternalCrate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    Contains,
    Uses,
    Calls,
    Implements,
    TypeReference,
    DataFlow,
    ModDeclaration,
    ExternalDependency,
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
