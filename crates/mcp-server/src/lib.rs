use anyhow::{Context, Result as AnyResult};
use clap::Parser;
use graph_builder::{build_fallback_graph, build_language_graph, filter_snapshot};
use graph_core::{
    AnalyzerCapability, AnalyzerEngine, AnalyzerKind, AnalyzerServiceStatus, AnalyzerStatus,
    AppState, AppStatus, DiagnosticRecord, DiagnosticSeverity, GraphEdge, GraphMode, GraphNode,
    GraphSnapshot, LanguageId, NodeDetailsResponse, NodeType, SearchResult, SourceReachability,
    SymbolIndex, TextPosition, TextRange,
};
use graph_query::context_pack::{
    build_edge_context_pack, build_node_context_pack, build_route_context_pack,
    build_trace_context_pack,
};
use graph_query::trace::{build_edge_trace, build_node_trace, build_route_trace};
use rmcp::model::{
    AnnotateAble, CallToolRequestParams, CallToolResult, Content, GetPromptRequestParams,
    GetPromptResult, Implementation, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument,
    PromptMessage, PromptMessageRole, RawResource, RawResourceTemplate, ReadResourceRequestParams,
    ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
};
use rmcp::service::{MaybeSendFuture, RequestContext, RoleServer};
use rmcp::{ErrorData as McpError, ServerHandler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, RwLock};
use std::time::Instant;

const MAX_GRAPH_NODES: usize = 500;
const MAX_GRAPH_EDGES: usize = 1_000;
const MAX_DIAGNOSTICS: usize = 100;
const MAX_DETACHED_FILES: usize = 200;
const CHECK_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Parser)]
#[command(name = "mcp-server")]
#[command(about = "Read-only MCP server for rust_watcher project graphs")]
pub struct Cli {
    #[arg(long)]
    pub project: Option<PathBuf>,
    #[arg(long, default_value = "rust-analyzer")]
    pub rust_analyzer: PathBuf,
    #[arg(long, default_value = "auto")]
    pub python_analyzer: String,
    #[arg(long, default_value = "ty")]
    pub ty_path: PathBuf,
    #[arg(long)]
    pub disable_ty: bool,
    #[arg(long, default_value = "auto")]
    pub typescript_analyzer: String,
    #[arg(long, default_value = "typescript-language-server")]
    pub typescript_language_server_path: PathBuf,
    #[arg(long)]
    pub disable_typescript_language_server: bool,
    #[arg(long, default_value = "auto")]
    pub qml_analyzer: String,
    #[arg(long, default_value = "qmlls")]
    pub qmlls_path: PathBuf,
    #[arg(long)]
    pub disable_qmlls: bool,
    #[arg(long)]
    pub qmlls_build_dir: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    pub qmlls_no_cmake_calls: bool,
}

#[derive(Debug, Clone)]
pub struct RustWatcherMcpServer {
    project_root: PathBuf,
    graph: Arc<GraphSnapshot>,
    diagnostics_by_file: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    diagnostics_by_node: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    check_results: Arc<RwLock<Vec<ProjectCheckResult>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub project_root: String,
    pub status: AppStatus,
    pub node_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    pub diagnostics_count: usize,
    pub checks: Vec<ProjectCheckResult>,
    pub safety: SafetyModel,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyModel {
    pub read_only: bool,
    pub exposes_file_mutation: bool,
    pub exposes_file_deletion: bool,
    pub exposes_shell_execution: bool,
    pub exposes_editor_open: bool,
    pub source_reads_stay_inside_project_root: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSnapshotResponse {
    pub mode: GraphMode,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub files_count: usize,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsResponse {
    pub diagnostics_by_file: HashMap<String, Vec<DiagnosticRecord>>,
    pub diagnostics_by_node: HashMap<String, Vec<DiagnosticRecord>>,
    pub all_diagnostics: Vec<DiagnosticRecord>,
    pub truncated: bool,
    pub checks: Vec<ProjectCheckResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCheckRunResponse {
    pub checks: Vec<ProjectCheckResult>,
    pub diagnostics: DiagnosticsResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCheckResult {
    pub id: String,
    pub label: String,
    pub command: Vec<String>,
    pub cwd: String,
    pub status: AnalyzerStatus,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub diagnostics_count: usize,
    pub duration_ms: u128,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachedRustFile {
    pub node_id: String,
    pub label: String,
    pub file: Option<String>,
    pub module: Option<String>,
    pub crate_name: Option<String>,
    pub detached_reason: Option<String>,
    pub reachable_from: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachedRustFilesResponse {
    pub files: Vec<DetachedRustFile>,
    pub total: usize,
    pub truncated: bool,
    pub warning: String,
}

#[derive(Debug, Deserialize)]
struct SearchSymbolsArgs {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct GraphSnapshotArgs {
    mode: GraphMode,
}

#[derive(Debug, Deserialize)]
struct NodeArgs {
    node_id: String,
}

#[derive(Debug, Deserialize)]
struct EdgeArgs {
    edge_id: String,
}

#[derive(Debug, Deserialize)]
struct RouteArgs {
    method: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct DiagnosticsArgs {
    severity: Option<DiagnosticSeverity>,
    language: Option<String>,
    #[serde(default = "default_diagnostics_limit")]
    limit: usize,
}

fn default_search_limit() -> usize {
    20
}

fn default_diagnostics_limit() -> usize {
    MAX_DIAGNOSTICS
}

impl RustWatcherMcpServer {
    pub fn build(args: Cli) -> AnyResult<Self> {
        let project_root = args
            .project
            .unwrap_or(std::env::current_dir().context("failed to read current directory")?)
            .canonicalize()
            .context("failed to canonicalize project root")?;
        let mut status = AppStatus::empty();
        status.app_state = AppState::Indexing;
        status.analyzer_status = AnalyzerStatus::Indexing;
        status.project_name = project_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        status.project_path = Some(project_root.display().to_string());
        status.message = Some("Indexing project for read-only MCP access".to_string());
        status.progress = Some(10);

        let mut snapshot = match project_indexer::index_project(&project_root) {
            Ok(index) => build_fallback_graph(&index, status),
            Err(error) => {
                let mut fallback = status;
                fallback.analyzer_status = AnalyzerStatus::Fallback;
                fallback.message = Some(format!(
                    "Cargo metadata unavailable ({error}); using language parser fallback"
                ));
                build_language_graph(&project_root, fallback)
            }
        };
        snapshot.status.app_state = AppState::Normal;
        snapshot.status.analyzer_status =
            if snapshot.status.analyzer_status == AnalyzerStatus::Fallback {
                AnalyzerStatus::Fallback
            } else {
                AnalyzerStatus::Ready
            };
        snapshot.status.progress = Some(100);
        snapshot.status.last_updated = Some(timestamp());
        if snapshot.status.message.is_none() {
            snapshot.status.message = Some("Ready".to_string());
        }
        snapshot.status.analyzers.push(cargo_check_service_status(
            AnalyzerStatus::Stale,
            "Checks not run yet",
            0,
        ));

        Ok(Self {
            project_root,
            graph: Arc::new(snapshot),
            diagnostics_by_file: Arc::new(RwLock::new(HashMap::new())),
            diagnostics_by_node: Arc::new(RwLock::new(HashMap::new())),
            check_results: Arc::new(RwLock::new(Vec::new())),
        })
    }

    pub fn from_snapshot(project_root: PathBuf, graph: GraphSnapshot) -> Self {
        Self {
            project_root,
            graph: Arc::new(graph),
            diagnostics_by_file: Arc::new(RwLock::new(HashMap::new())),
            diagnostics_by_node: Arc::new(RwLock::new(HashMap::new())),
            check_results: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn status_response(&self) -> StatusResponse {
        let diagnostics_by_file = self.diagnostics_by_file.read().unwrap();
        let checks = self.check_results.read().unwrap().clone();
        let diagnostics_count = diagnostics_by_file.values().map(Vec::len).sum::<usize>();
        let mut status = self.graph.status.clone();
        status
            .analyzers
            .retain(|service| service.id != "cargo-check");
        status.analyzers.push(check_service_from_results(
            &checks,
            diagnostics_count as u32,
        ));
        StatusResponse {
            project_root: self.project_root.display().to_string(),
            status,
            node_count: self.graph.nodes.len(),
            edge_count: self.graph.edges.len(),
            file_count: self.graph.files.len(),
            diagnostics_count,
            checks,
            safety: SafetyModel {
                read_only: true,
                exposes_file_mutation: false,
                exposes_file_deletion: false,
                exposes_shell_execution: false,
                exposes_editor_open: false,
                source_reads_stay_inside_project_root: true,
            },
        }
    }

    pub fn search_symbols(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        graph_query::search_nodes(&self.graph, query, limit.min(100))
    }

    pub fn graph_snapshot(&self, mode: GraphMode) -> GraphSnapshotResponse {
        compact_graph_response(filter_snapshot(&self.graph, mode), mode)
    }

    pub fn node_details(&self, node_id: &str) -> Option<NodeDetailsResponse> {
        let node_by_id = self
            .graph
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect::<HashMap<_, _>>();
        let incoming_edges = self
            .graph
            .edges
            .iter()
            .filter(|edge| edge.target == node_id)
            .cloned()
            .collect::<Vec<_>>();
        let mut references = graph_query::graph_reference_records(&incoming_edges, &node_by_id);
        graph_query::dedupe_references(&mut references);
        let diagnostics = self
            .diagnostics_by_node
            .read()
            .unwrap()
            .get(node_id)
            .cloned()
            .unwrap_or_default();
        graph_query::node_details_base(&self.graph, node_id, diagnostics, references)
    }

    pub fn node_context(&self, node_id: &str) -> Option<graph_core::ContextPack> {
        let node = self.graph.nodes.iter().find(|node| node.id == node_id)?;
        let diagnostics_by_node = self.diagnostics_by_node.read().unwrap();
        Some(build_node_context_pack(
            &self.graph,
            &self.project_root,
            &diagnostics_by_node,
            node,
        ))
    }

    pub fn edge_context(&self, edge_id: &str) -> Option<graph_core::ContextPack> {
        let edge = self.graph.edges.iter().find(|edge| edge.id == edge_id)?;
        let diagnostics_by_node = self.diagnostics_by_node.read().unwrap();
        Some(build_edge_context_pack(
            &self.graph,
            &self.project_root,
            &diagnostics_by_node,
            edge,
        ))
    }

    pub fn trace_node(&self, node_id: &str) -> Option<graph_core::TraceExplanation> {
        let node = self.graph.nodes.iter().find(|node| node.id == node_id)?;
        Some(build_node_trace(&self.graph, node))
    }

    pub fn trace_route(&self, method: &str, path: &str) -> Option<graph_core::TraceExplanation> {
        let requested = graph_core::route_key(method, path).key;
        let endpoint = graph_query::find_active_endpoint_by_route_key(&self.graph, &requested)?;
        Some(build_route_trace(&self.graph, endpoint))
    }

    pub fn route_context(&self, method: &str, path: &str) -> Option<graph_core::ContextPack> {
        let requested = graph_core::route_key(method, path).key;
        let endpoint = graph_query::find_active_endpoint_by_route_key(&self.graph, &requested)?;
        let diagnostics_by_node = self.diagnostics_by_node.read().unwrap();
        Some(build_route_context_pack(
            &self.graph,
            &self.project_root,
            &diagnostics_by_node,
            endpoint,
        ))
    }

    pub fn trace_context(&self, node_id: &str) -> Option<graph_core::ContextPack> {
        let trace = self.trace_node(node_id)?;
        let diagnostics_by_node = self.diagnostics_by_node.read().unwrap();
        Some(build_trace_context_pack(
            &self.graph,
            &self.project_root,
            &diagnostics_by_node,
            &trace,
        ))
    }

    pub fn run_project_checks(&self) -> ProjectCheckRunResponse {
        let check_run = run_cargo_checks(&self.project_root, &self.graph);
        *self.diagnostics_by_file.write().unwrap() = check_run.diagnostics_by_file;
        *self.diagnostics_by_node.write().unwrap() = check_run.diagnostics_by_node;
        *self.check_results.write().unwrap() = check_run.results;
        ProjectCheckRunResponse {
            checks: self.check_results.read().unwrap().clone(),
            diagnostics: self.diagnostics(None, None, MAX_DIAGNOSTICS),
        }
    }

    pub fn edge_trace(&self, edge_id: &str) -> Option<graph_core::TraceExplanation> {
        let edge = self.graph.edges.iter().find(|edge| edge.id == edge_id)?;
        Some(build_edge_trace(&self.graph, edge))
    }

    pub fn diagnostics(
        &self,
        severity: Option<DiagnosticSeverity>,
        language: Option<&str>,
        limit: usize,
    ) -> DiagnosticsResponse {
        let diagnostics_by_file = self.diagnostics_by_file.read().unwrap();
        let mut all = diagnostics_by_file
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        drop(diagnostics_by_file);
        all.retain(|diagnostic| severity.is_none_or(|severity| diagnostic.severity == severity));
        all.retain(|diagnostic| {
            language.is_none_or(|language| diagnostic.language == LanguageId::from(language))
        });
        all.sort_by(|left, right| left.file.cmp(&right.file).then(left.id.cmp(&right.id)));
        let truncated = all.len() > limit;
        all.truncate(limit);
        let diagnostics_by_file = group_diagnostics_by_file(&all);
        let diagnostics_by_node = group_diagnostics_by_node(&all);
        DiagnosticsResponse {
            diagnostics_by_file,
            diagnostics_by_node,
            all_diagnostics: all,
            truncated,
            checks: self.check_results.read().unwrap().clone(),
        }
    }

    pub fn detached_rust_files(&self) -> DetachedRustFilesResponse {
        let mut files = self
            .graph
            .nodes
            .iter()
            .filter(|node| node.node_type == NodeType::File)
            .filter(|node| node.language.as_deref() == Some(LanguageId::Rust.as_str()))
            .filter(|node| node.reachability == Some(SourceReachability::Detached))
            .map(|node| DetachedRustFile {
                node_id: node.id.clone(),
                label: node.label.clone(),
                file: node.file.clone(),
                module: node.module.clone(),
                crate_name: node.crate_name.clone(),
                detached_reason: node.detached_reason.clone(),
                reachable_from: node.reachable_from.clone(),
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| {
            left.file
                .cmp(&right.file)
                .then(left.label.cmp(&right.label))
        });
        let total = files.len();
        let truncated = total > MAX_DETACHED_FILES;
        files.truncate(MAX_DETACHED_FILES);
        DetachedRustFilesResponse {
            files,
            total,
            truncated,
            warning:
                "Detached means not reachable from active Rust crate roots/mod declarations; it is not proof that code is safe to delete."
                    .to_string(),
        }
    }

    fn read_resource_value(&self, uri: &str) -> Option<Value> {
        if uri == "watcher://status" {
            return Some(json!(self.status_response()));
        }
        if uri == "watcher://diagnostics" {
            return Some(json!(self.diagnostics(None, None, MAX_DIAGNOSTICS)));
        }
        if uri == "watcher://detached/rust" {
            return Some(json!(self.detached_rust_files()));
        }
        if let Some(mode) = uri
            .strip_prefix("watcher://graph/snapshot?mode=")
            .and_then(parse_graph_mode)
        {
            return Some(json!(self.graph_snapshot(mode)));
        }
        if let Some(query) = uri.strip_prefix("watcher://search?q=") {
            return Some(json!({ "results": self.search_symbols(query, 30) }));
        }
        if let Some(node_id) = uri.strip_prefix("watcher://node/") {
            return self.node_details(node_id).map(|details| json!(details));
        }
        if let Some(node_id) = uri.strip_prefix("watcher://context/node/") {
            return self.node_context(node_id).map(|pack| json!(pack));
        }
        if let Some(node_id) = uri.strip_prefix("watcher://trace/node/") {
            return self.trace_node(node_id).map(|trace| json!(trace));
        }
        None
    }

    fn tools(&self) -> Vec<Tool> {
        vec![
            tool(
                "get_status",
                "Return project indexing/analyzer status.",
                empty_object_schema(),
            ),
            tool(
                "search_symbols",
                "Search files, symbols, routes, and components by label/file/module.",
                object_schema(
                    [
                        ("query", json!({"type": "string"})),
                        (
                            "limit",
                            json!({"type": "integer", "default": 20, "minimum": 1, "maximum": 100}),
                        ),
                    ],
                    ["query"],
                ),
            ),
            tool(
                "get_graph_snapshot",
                "Return a bounded graph snapshot for a graph mode.",
                object_schema(
                    [(
                        "mode",
                        enum_schema(&["Macro", "Meso", "Micro", "CallFlow", "DataFlow", "Traits"]),
                    )],
                    ["mode"],
                ),
            ),
            tool(
                "get_node",
                "Return node details, connected edges, diagnostics, and graph references.",
                object_schema([("node_id", json!({"type": "string"}))], ["node_id"]),
            ),
            tool(
                "get_node_context",
                "Return a bounded source/context pack for a node.",
                object_schema([("node_id", json!({"type": "string"}))], ["node_id"]),
            ),
            tool(
                "get_edge_context",
                "Return a bounded source/context pack for an edge.",
                object_schema([("edge_id", json!({"type": "string"}))], ["edge_id"]),
            ),
            tool(
                "trace_node",
                "Trace a node neighborhood, route, or data-flow path when supported.",
                object_schema([("node_id", json!({"type": "string"}))], ["node_id"]),
            ),
            tool(
                "trace_route",
                "Trace a frontend/API route to backend endpoint/handler when present in the graph.",
                object_schema(
                    [
                        ("method", json!({"type": "string"})),
                        ("path", json!({"type": "string"})),
                    ],
                    ["method", "path"],
                ),
            ),
            tool(
                "list_diagnostics",
                "Return diagnostics grouped by file/node.",
                object_schema(
                    [
                        (
                            "severity",
                            enum_or_null_schema(&["Error", "Warning", "Information", "Hint"]),
                        ),
                        ("language", json!({"type": ["string", "null"]})),
                        (
                            "limit",
                            json!({"type": "integer", "default": 100, "minimum": 1, "maximum": 100}),
                        ),
                    ],
                    [],
                ),
            ),
            tool(
                "run_project_checks",
                "Run fixed read-only project checks and refresh diagnostics.",
                empty_object_schema(),
            ),
            tool(
                "list_detached_rust_files",
                "Return Rust file nodes marked SourceReachability::Detached.",
                empty_object_schema(),
            ),
        ]
    }

    fn resources(&self) -> Vec<rmcp::model::Resource> {
        [
            ("watcher://status", "status", "Project status"),
            (
                "watcher://graph/snapshot?mode=Macro",
                "graph-macro",
                "Macro graph snapshot",
            ),
            (
                "watcher://graph/snapshot?mode=Meso",
                "graph-meso",
                "Meso graph snapshot",
            ),
            (
                "watcher://graph/snapshot?mode=Micro",
                "graph-micro",
                "Micro graph snapshot",
            ),
            (
                "watcher://graph/snapshot?mode=CallFlow",
                "graph-call-flow",
                "Call-flow graph snapshot",
            ),
            (
                "watcher://graph/snapshot?mode=DataFlow",
                "graph-data-flow",
                "Data-flow graph snapshot",
            ),
            (
                "watcher://graph/snapshot?mode=Traits",
                "graph-traits",
                "Traits graph snapshot",
            ),
            ("watcher://diagnostics", "diagnostics", "Diagnostics"),
            (
                "watcher://detached/rust",
                "detached-rust",
                "Detached Rust files",
            ),
        ]
        .into_iter()
        .map(|(uri, name, description)| {
            RawResource::new(uri, name)
                .with_description(description)
                .with_mime_type("application/json")
                .no_annotation()
        })
        .collect()
    }

    fn resource_templates(&self) -> Vec<rmcp::model::ResourceTemplate> {
        [
            (
                "watcher://search?q={query}",
                "search",
                "Search symbols by query",
            ),
            ("watcher://node/{node_id}", "node", "Node details"),
            (
                "watcher://context/node/{node_id}",
                "node-context",
                "Node context pack",
            ),
            (
                "watcher://trace/node/{node_id}",
                "node-trace",
                "Node trace explanation",
            ),
        ]
        .into_iter()
        .map(|(uri, name, description)| {
            RawResourceTemplate::new(uri, name)
                .with_description(description)
                .with_mime_type("application/json")
                .no_annotation()
        })
        .collect()
    }

    fn prompts(&self) -> Vec<Prompt> {
        vec![
            Prompt::new(
                "explain_architecture",
                Some("Use rust_watcher graph resources to explain project architecture."),
                None,
            ),
            Prompt::new(
                "review_change_impact",
                Some("Find a symbol or file and inspect likely change impact."),
                Some(vec![
                    PromptArgument::new("symbol_or_file").with_required(true)
                ]),
            ),
            Prompt::new(
                "explain_frontend_to_backend_route",
                Some("Trace a route from frontend API usage to backend endpoint/handler."),
                Some(vec![
                    PromptArgument::new("method").with_required(true),
                    PromptArgument::new("path").with_required(true),
                ]),
            ),
            Prompt::new(
                "find_unused_or_detached_code",
                Some("Inspect detached Rust files and low-connectivity nodes cautiously."),
                None,
            ),
        ]
    }
}

impl ServerHandler for RustWatcherMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new(
            "rust_watcher-mcp-server",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions("Read-only access to rust_watcher graph snapshots, context packs, traces, diagnostics, and detached-file metadata.")
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + MaybeSendFuture + '_
    {
        std::future::ready(Ok(ListToolsResult::with_all_items(self.tools())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools().into_iter().find(|tool| tool.name == name)
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, McpError>> + MaybeSendFuture + '_
    {
        std::future::ready(self.call_tool_inner(request))
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, McpError>> + MaybeSendFuture + '_
    {
        std::future::ready(Ok(ListResourcesResult::with_all_items(self.resources())))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourceTemplatesResult, McpError>>
           + MaybeSendFuture
           + '_ {
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(
            self.resource_templates(),
        )))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResult, McpError>> + MaybeSendFuture + '_
    {
        std::future::ready(match self.read_resource_value(&request.uri) {
            Some(value) => Ok(ReadResourceResult::new(vec![ResourceContents::text(
                value.to_string(),
                request.uri,
            )
            .with_mime_type("application/json")])),
            None => Err(McpError::resource_not_found("resource not found", None)),
        })
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListPromptsResult, McpError>> + MaybeSendFuture + '_
    {
        std::future::ready(Ok(ListPromptsResult::with_all_items(self.prompts())))
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetPromptResult, McpError>> + MaybeSendFuture + '_
    {
        std::future::ready(self.get_prompt_inner(request))
    }
}

impl RustWatcherMcpServer {
    fn call_tool_inner(&self, request: CallToolRequestParams) -> Result<CallToolResult, McpError> {
        match request.name.as_ref() {
            "get_status" => Ok(structured(self.status_response())),
            "search_symbols" => {
                let args: SearchSymbolsArgs = decode_args(request.arguments)?;
                Ok(structured(json!({
                    "results": self.search_symbols(&args.query, args.limit)
                })))
            }
            "get_graph_snapshot" => {
                let args: GraphSnapshotArgs = decode_args(request.arguments)?;
                Ok(structured(self.graph_snapshot(args.mode)))
            }
            "get_node" => {
                let args: NodeArgs = decode_args(request.arguments)?;
                match self.node_details(&args.node_id) {
                    Some(details) => Ok(structured(details)),
                    None => Ok(tool_error(format!("node not found: {}", args.node_id))),
                }
            }
            "get_node_context" => {
                let args: NodeArgs = decode_args(request.arguments)?;
                match self.node_context(&args.node_id) {
                    Some(pack) => Ok(structured(pack)),
                    None => Ok(tool_error(format!("node not found: {}", args.node_id))),
                }
            }
            "get_edge_context" => {
                let args: EdgeArgs = decode_args(request.arguments)?;
                match self.edge_context(&args.edge_id) {
                    Some(pack) => Ok(structured(pack)),
                    None => Ok(tool_error(format!("edge not found: {}", args.edge_id))),
                }
            }
            "trace_node" => {
                let args: NodeArgs = decode_args(request.arguments)?;
                match self.trace_node(&args.node_id) {
                    Some(trace) => Ok(structured(trace)),
                    None => Ok(tool_error(format!("node not found: {}", args.node_id))),
                }
            }
            "trace_route" => {
                let args: RouteArgs = decode_args(request.arguments)?;
                match self.trace_route(&args.method, &args.path) {
                    Some(trace) => Ok(structured(trace)),
                    None => Ok(tool_error(format!(
                        "active route not found: {} {}",
                        args.method, args.path
                    ))),
                }
            }
            "list_diagnostics" => {
                let args: DiagnosticsArgs = decode_args(request.arguments)?;
                Ok(structured(self.diagnostics(
                    args.severity,
                    args.language.as_deref(),
                    args.limit,
                )))
            }
            "run_project_checks" => Ok(structured(self.run_project_checks())),
            "list_detached_rust_files" => Ok(structured(self.detached_rust_files())),
            other => Err(McpError::invalid_params(
                format!("unknown tool: {other}"),
                None,
            )),
        }
    }

    fn get_prompt_inner(
        &self,
        request: GetPromptRequestParams,
    ) -> Result<GetPromptResult, McpError> {
        let args = request.arguments.unwrap_or_default();
        let text = match request.name.as_str() {
            "explain_architecture" => "Use get_status first, then inspect get_graph_snapshot for Macro and Meso. Search for entrypoints, routes, major modules, and explain architecture from graph evidence. Ask focused follow-up calls instead of loading entire files.".to_string(),
            "review_change_impact" => {
                let symbol = string_arg(&args, "symbol_or_file")?;
                format!("Use search_symbols for '{symbol}', then inspect get_node, get_node_context, trace_node, callers/callees/references, and nearby diagnostics. Summarize likely impact and uncertainty.")
            }
            "explain_frontend_to_backend_route" => {
                let method = string_arg(&args, "method")?;
                let path = string_arg(&args, "path")?;
                format!("Use trace_route with method '{method}' and path '{path}', then inspect route/node context packs for frontend callers, endpoint handlers, and data-flow evidence.")
            }
            "find_unused_or_detached_code" => "Use list_detached_rust_files and graph snapshots to inspect detached Rust files and low-connectivity nodes. Explicitly warn that detached does not always mean safe to delete; verify build targets, feature flags, generated code, and external references before recommending deletion.".to_string(),
            other => {
                return Err(McpError::invalid_params(
                    format!("unknown prompt: {other}"),
                    None,
                ))
            }
        };
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            text,
        )]))
    }
}

fn compact_graph_response(snapshot: GraphSnapshot, mode: GraphMode) -> GraphSnapshotResponse {
    let total_nodes = snapshot.nodes.len();
    let total_edges = snapshot.edges.len();
    let mut nodes = snapshot.nodes;
    let mut edges = snapshot.edges;
    let truncated = total_nodes > MAX_GRAPH_NODES || total_edges > MAX_GRAPH_EDGES;
    nodes.truncate(MAX_GRAPH_NODES);
    let kept_node_ids = nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    edges.retain(|edge| {
        kept_node_ids.contains(edge.source.as_str()) && kept_node_ids.contains(edge.target.as_str())
    });
    edges.truncate(MAX_GRAPH_EDGES);
    let mut warnings = Vec::new();
    if truncated {
        warnings.push(format!(
            "Graph response truncated to {MAX_GRAPH_NODES} nodes and {MAX_GRAPH_EDGES} edges; use search/context/trace tools for follow-up."
        ));
    }
    GraphSnapshotResponse {
        mode,
        nodes,
        edges,
        total_nodes,
        total_edges,
        files_count: snapshot.files.len(),
        truncated,
        warnings,
    }
}

struct ProjectCheckRun {
    results: Vec<ProjectCheckResult>,
    diagnostics_by_file: HashMap<String, Vec<DiagnosticRecord>>,
    diagnostics_by_node: HashMap<String, Vec<DiagnosticRecord>>,
}

#[derive(Debug, Deserialize)]
struct CargoJsonMessage {
    reason: String,
    message: Option<CargoCompilerMessage>,
}

#[derive(Debug, Deserialize)]
struct CargoCompilerMessage {
    message: String,
    code: Option<CargoDiagnosticCode>,
    level: String,
    spans: Vec<CargoDiagnosticSpan>,
}

#[derive(Debug, Deserialize)]
struct CargoDiagnosticCode {
    code: String,
}

#[derive(Debug, Deserialize)]
struct CargoDiagnosticSpan {
    file_name: String,
    line_start: u32,
    line_end: u32,
    column_start: u32,
    column_end: u32,
    is_primary: bool,
}

fn run_cargo_checks(project_root: &Path, graph: &GraphSnapshot) -> ProjectCheckRun {
    let cargo_projects = discover_cargo_projects(project_root);
    let symbol_index = SymbolIndex::from_nodes(&graph.nodes);
    let mut diagnostics_by_file = HashMap::<String, Vec<DiagnosticRecord>>::new();
    let mut results = Vec::new();

    if cargo_projects.is_empty() {
        results.push(ProjectCheckResult {
            id: "cargo-check:none".to_string(),
            label: "cargo check".to_string(),
            command: vec!["cargo".to_string(), "check".to_string()],
            cwd: project_root.display().to_string(),
            status: AnalyzerStatus::Fallback,
            success: false,
            exit_code: None,
            diagnostics_count: 0,
            duration_ms: 0,
            message: Some("No Cargo.toml found under project root.".to_string()),
        });
    }

    for manifest_dir in cargo_projects {
        let started = Instant::now();
        let mut command = Command::new("cargo");
        command
            .arg("check")
            .arg("--message-format=json")
            .arg("--all-targets")
            .current_dir(&manifest_dir)
            .env("CARGO_TERM_COLOR", "never")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let relative_dir = project_indexer::relative_to(project_root, &manifest_dir);
        let check_id = format!("cargo-check:{relative_dir}");
        let command_display = vec![
            "cargo".to_string(),
            "check".to_string(),
            "--message-format=json".to_string(),
            "--all-targets".to_string(),
        ];

        let output = match run_command_with_timeout(command, CHECK_TIMEOUT_SECS) {
            Ok(output) => output,
            Err(error) => {
                results.push(ProjectCheckResult {
                    id: check_id,
                    label: format!("cargo check ({relative_dir})"),
                    command: command_display,
                    cwd: manifest_dir.display().to_string(),
                    status: AnalyzerStatus::Error,
                    success: false,
                    exit_code: None,
                    diagnostics_count: 0,
                    duration_ms: started.elapsed().as_millis(),
                    message: Some(error),
                });
                continue;
            }
        };

        let diagnostics = parse_cargo_diagnostics(
            project_root,
            &manifest_dir,
            &symbol_index,
            &String::from_utf8_lossy(&output.stdout),
        );
        let diagnostics_count = diagnostics.len();
        for diagnostic in diagnostics {
            diagnostics_by_file
                .entry(diagnostic.file.clone())
                .or_default()
                .push(diagnostic);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if output.timed_out {
            Some(format!("cargo check timed out after {CHECK_TIMEOUT_SECS}s"))
        } else if output.status_success {
            Some("cargo check passed".to_string())
        } else if stderr.trim().is_empty() {
            Some("cargo check failed".to_string())
        } else {
            Some(stderr.trim().lines().take(3).collect::<Vec<_>>().join("\n"))
        };

        results.push(ProjectCheckResult {
            id: check_id,
            label: format!("cargo check ({relative_dir})"),
            command: command_display,
            cwd: manifest_dir.display().to_string(),
            status: if output.status_success {
                AnalyzerStatus::Ready
            } else {
                AnalyzerStatus::Error
            },
            success: output.status_success,
            exit_code: output.exit_code,
            diagnostics_count,
            duration_ms: started.elapsed().as_millis(),
            message,
        });
    }

    let all_diagnostics = diagnostics_by_file
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    let diagnostics_by_node = group_diagnostics_by_node(&all_diagnostics);

    ProjectCheckRun {
        results,
        diagnostics_by_file,
        diagnostics_by_node,
    }
}

struct CommandOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status_success: bool,
    exit_code: Option<i32>,
    timed_out: bool,
}

fn run_command_with_timeout(
    mut command: Command,
    timeout_secs: u64,
) -> Result<CommandOutput, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start cargo check: {error}"))?;
    let started = Instant::now();
    let mut timed_out = false;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed().as_secs() >= timeout_secs {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(error) => return Err(format!("failed to wait for cargo check: {error}")),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to collect cargo check output: {error}"))?;
    Ok(CommandOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        status_success: output.status.success() && !timed_out,
        exit_code: output.status.code(),
        timed_out,
    })
}

fn parse_cargo_diagnostics(
    project_root: &Path,
    manifest_dir: &Path,
    symbol_index: &SymbolIndex,
    stdout: &str,
) -> Vec<DiagnosticRecord> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<CargoJsonMessage>(line).ok())
        .filter(|message| message.reason == "compiler-message")
        .filter_map(|message| message.message)
        .enumerate()
        .filter_map(|(index, message)| {
            let span = message
                .spans
                .iter()
                .find(|span| span.is_primary)
                .or_else(|| message.spans.first())?;
            let file = normalize_cargo_file(project_root, manifest_dir, &span.file_name);
            let range = cargo_span_range(span);
            let related_node_ids = related_nodes_for_range(symbol_index, &file, range);
            Some(DiagnosticRecord {
                id: format!(
                    "diagnostic:cargo-check:{file}:{}:{}:{index}",
                    range.start.line, range.start.character
                ),
                language: language_for_file(&file),
                file,
                range: Some(range),
                severity: cargo_severity(&message.level),
                source: Some("cargo check".to_string()),
                message: message.message,
                code: message.code.map(|code| code.code),
                related_node_ids,
            })
        })
        .collect()
}

fn discover_cargo_projects(project_root: &Path) -> Vec<PathBuf> {
    let mut manifests = Vec::new();
    discover_cargo_projects_inner(project_root, &mut manifests);
    manifests.sort();
    manifests.dedup();
    if project_root.join("Cargo.toml").exists()
        && cargo_manifest_is_workspace(&project_root.join("Cargo.toml"))
    {
        return vec![project_root.to_path_buf()];
    }
    manifests
}

fn discover_cargo_projects_inner(dir: &Path, manifests: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    if dir.join("Cargo.toml").exists() {
        manifests.push(dir.to_path_buf());
    }
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || should_skip_check_dir(&path) {
            continue;
        }
        discover_cargo_projects_inner(&path, manifests);
    }
}

fn should_skip_check_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".git" | "target" | "node_modules" | "dist" | ".next" | ".venv" | "venv")
    )
}

fn cargo_manifest_is_workspace(manifest: &Path) -> bool {
    std::fs::read_to_string(manifest)
        .map(|content| content.lines().any(|line| line.trim() == "[workspace]"))
        .unwrap_or(false)
}

fn normalize_cargo_file(project_root: &Path, manifest_dir: &Path, file_name: &str) -> String {
    let raw = Path::new(file_name);
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        manifest_dir.join(raw)
    };
    absolute
        .strip_prefix(project_root)
        .map(normalize_path)
        .unwrap_or_else(|_| normalize_path(raw))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn cargo_span_range(span: &CargoDiagnosticSpan) -> TextRange {
    TextRange {
        start: TextPosition {
            line: span.line_start.saturating_sub(1),
            character: span.column_start.saturating_sub(1),
        },
        end: TextPosition {
            line: span.line_end.saturating_sub(1),
            character: span.column_end.saturating_sub(1),
        },
    }
}

fn cargo_severity(level: &str) -> DiagnosticSeverity {
    match level {
        "error" => DiagnosticSeverity::Error,
        "warning" => DiagnosticSeverity::Warning,
        "help" => DiagnosticSeverity::Hint,
        "note" => DiagnosticSeverity::Information,
        _ => DiagnosticSeverity::Information,
    }
}

fn language_for_file(file: &str) -> LanguageId {
    match Path::new(file)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("ts" | "tsx") => LanguageId::TypeScript,
        Some("js" | "jsx") => LanguageId::JavaScript,
        Some("py") => LanguageId::Python,
        Some("qml") => LanguageId::Qml,
        _ => LanguageId::Rust,
    }
}

fn related_nodes_for_range(
    symbol_index: &SymbolIndex,
    file: &str,
    range: TextRange,
) -> Vec<String> {
    symbol_index
        .find_by_file(file)
        .into_iter()
        .filter(|symbol| ranges_overlap(symbol.range, range))
        .map(|symbol| symbol.node_id.clone())
        .collect()
}

fn ranges_overlap(left: TextRange, right: TextRange) -> bool {
    position_le(left.start, right.end) && position_le(right.start, left.end)
}

fn position_le(left: TextPosition, right: TextPosition) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
}

fn decode_args<T: for<'de> Deserialize<'de>>(
    arguments: Option<Map<String, Value>>,
) -> Result<T, McpError> {
    serde_json::from_value(Value::Object(arguments.unwrap_or_default()))
        .map_err(|error| McpError::invalid_params(error.to_string(), None))
}

fn structured(value: impl Serialize) -> CallToolResult {
    CallToolResult::structured(serde_json::to_value(value).unwrap_or_else(|error| {
        json!({
            "error": format!("failed to serialize response: {error}")
        })
    }))
}

fn tool_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(message)])
}

fn tool(name: &'static str, description: &'static str, input_schema: Map<String, Value>) -> Tool {
    let mut tool = Tool::new(name, description, input_schema);
    tool.annotations = Some(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );
    tool
}

fn object_schema<const N: usize, const R: usize>(
    properties: [(&str, Value); N],
    required: [&str; R],
) -> Map<String, Value> {
    let mut property_map = Map::new();
    for (name, schema) in properties {
        property_map.insert(name.to_string(), schema);
    }
    let mut schema = Map::new();
    schema.insert("type".to_string(), json!("object"));
    schema.insert("properties".to_string(), Value::Object(property_map));
    if !required.is_empty() {
        schema.insert("required".to_string(), json!(required.to_vec()));
    }
    schema
}

fn empty_object_schema() -> Map<String, Value> {
    object_schema([], [])
}

fn enum_schema(values: &[&str]) -> Value {
    json!({
        "type": "string",
        "enum": values,
    })
}

fn enum_or_null_schema(values: &[&str]) -> Value {
    json!({
        "type": ["string", "null"],
        "enum": values.iter().copied().map(Value::from).chain(std::iter::once(Value::Null)).collect::<Vec<_>>(),
    })
}

fn parse_graph_mode(value: &str) -> Option<GraphMode> {
    match value {
        "Macro" => Some(GraphMode::Macro),
        "Meso" => Some(GraphMode::Meso),
        "Micro" => Some(GraphMode::Micro),
        "CallFlow" => Some(GraphMode::CallFlow),
        "DataFlow" => Some(GraphMode::DataFlow),
        "Traits" => Some(GraphMode::Traits),
        _ => None,
    }
}

fn group_diagnostics_by_file(
    diagnostics: &[DiagnosticRecord],
) -> HashMap<String, Vec<DiagnosticRecord>> {
    let mut grouped = HashMap::new();
    for diagnostic in diagnostics {
        grouped
            .entry(diagnostic.file.clone())
            .or_insert_with(Vec::new)
            .push(diagnostic.clone());
    }
    grouped
}

fn group_diagnostics_by_node(
    diagnostics: &[DiagnosticRecord],
) -> HashMap<String, Vec<DiagnosticRecord>> {
    let mut grouped = HashMap::new();
    for diagnostic in diagnostics {
        for node_id in &diagnostic.related_node_ids {
            grouped
                .entry(node_id.clone())
                .or_insert_with(Vec::new)
                .push(diagnostic.clone());
        }
    }
    grouped
}

fn cargo_check_service_status(
    status: AnalyzerStatus,
    message: &str,
    files_indexed: u32,
) -> AnalyzerServiceStatus {
    AnalyzerServiceStatus {
        id: "cargo-check".to_string(),
        kind: AnalyzerKind::Rust,
        engine: AnalyzerEngine::Other,
        label: "cargo check".to_string(),
        status,
        mode: Some("fixed-command".to_string()),
        message: Some(message.to_string()),
        capabilities: vec![AnalyzerCapability::Diagnostics],
        files_indexed,
        last_updated: Some(timestamp()),
    }
}

fn check_service_from_results(
    checks: &[ProjectCheckResult],
    diagnostics_count: u32,
) -> AnalyzerServiceStatus {
    if checks.is_empty() {
        return cargo_check_service_status(AnalyzerStatus::Stale, "Checks not run yet", 0);
    }
    let status = if checks
        .iter()
        .any(|check| check.status == AnalyzerStatus::Error)
    {
        AnalyzerStatus::Error
    } else if checks
        .iter()
        .any(|check| check.status == AnalyzerStatus::Fallback)
    {
        AnalyzerStatus::Fallback
    } else {
        AnalyzerStatus::Ready
    };
    let passed = checks.iter().filter(|check| check.success).count();
    cargo_check_service_status(
        status,
        &format!(
            "{passed}/{} cargo check command(s) passed; {diagnostics_count} diagnostic(s).",
            checks.len()
        ),
        diagnostics_count,
    )
}

fn string_arg(arguments: &Map<String, Value>, name: &str) -> Result<String, McpError> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| McpError::invalid_params(format!("missing string argument: {name}"), None))
}

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
