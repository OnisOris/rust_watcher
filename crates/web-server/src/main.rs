use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path as AxumPath, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use graph_builder::{filter_snapshot, python, qml, typescript};
use graph_core::{
    AnalysisEvent, AnalysisEventType, AnalyzerCapability, AnalyzerEngine, AnalyzerKind,
    AnalyzerProvider, AnalyzerServiceStatus, AnalyzerStatus, AppState, AppStatus, DiagnosticRecord,
    DiagnosticSeverity, FocusDepth, FocusRequest, GraphMode, GraphNode, GraphPatch, GraphSnapshot,
    LanguageId, PythonAnalyzerStatus, ReferenceRecord, SearchResult, ServerMessage, SourceLocation,
    SymbolIndex,
};
use parking_lot::RwLock;
use project_indexer::start_watcher;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::broadcast;
use tokio::time::{sleep, timeout, Duration};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

mod analysis;
mod analyzer_paths;
mod lsp_runtime;
mod python_ty;
mod qml_lsp;
mod typescript_lsp;
use analyzer_paths::resolve_rust_analyzer;
use graph_query::context_pack::{
    build_edge_context_pack, build_node_context_pack, build_route_context_pack,
    build_trace_context_pack,
};
use graph_query::trace::{build_edge_trace, build_node_trace, build_route_trace};
use lsp_runtime::{LspRuntime, LspRuntimeConfig, LspRuntimeMode};
use python_ty::{PythonAnalyzerMode, PythonTyState};
use qml_lsp::{
    status_to_analyzer_status as qml_status_to_analyzer_status, QmlAnalyzerMode, QmlAnalyzerStatus,
    QmlLspState,
};
use typescript_lsp::{
    language_for_path, locations_from_definition_response, status_to_analyzer_status,
    TypeScriptAnalyzerMode, TypeScriptAnalyzerStatus, TypeScriptLspState,
};

#[derive(Parser)]
#[command(name = "rust-code-command-center")]
#[command(about = "Local browser command center for Rust project graphs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Serve(ServeArgs),
}

#[derive(Parser, Clone)]
struct ServeArgs {
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long)]
    open: bool,
    #[arg(long, default_value = "frontend/dist")]
    frontend_dist: PathBuf,
    #[arg(long, default_value = "rust-analyzer")]
    rust_analyzer: PathBuf,
    #[arg(long)]
    enable_editor_open: bool,
    #[arg(long, value_enum, default_value_t = PythonAnalyzerMode::Auto)]
    python_analyzer: PythonAnalyzerMode,
    #[arg(long, default_value = "ty")]
    ty_path: PathBuf,
    #[arg(long)]
    disable_ty: bool,
    #[arg(long, value_enum, default_value_t = TypeScriptAnalyzerMode::Auto)]
    typescript_analyzer: TypeScriptAnalyzerMode,
    #[arg(long, default_value = "typescript-language-server")]
    typescript_language_server_path: PathBuf,
    #[arg(long)]
    disable_typescript_language_server: bool,
    #[arg(long, value_enum, default_value_t = QmlAnalyzerMode::Auto)]
    qml_analyzer: QmlAnalyzerMode,
    #[arg(long, default_value = "qmlls")]
    qmlls_path: PathBuf,
    #[arg(long)]
    disable_qmlls: bool,
    #[arg(long)]
    qmlls_build_dir: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    qmlls_no_cmake_calls: bool,
}

#[derive(Clone)]
struct AppStateHandle {
    project_root: Arc<RwLock<PathBuf>>,
    graph: Arc<RwLock<GraphSnapshot>>,
    status: Arc<RwLock<AppStatus>>,
    ws_tx: broadcast::Sender<ServerMessage>,
    analyzer: Arc<AnalyzerState>,
    python_ty: Arc<PythonTyState>,
    typescript_lsp: Arc<TypeScriptLspState>,
    qml_lsp: Arc<QmlLspState>,
    diagnostics_by_file: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    diagnostics_by_node: Arc<RwLock<HashMap<String, Vec<DiagnosticRecord>>>>,
    watcher: Arc<RwLock<Option<notify::RecommendedWatcher>>>,
    is_indexing: Arc<AtomicBool>,
    enable_editor_open: bool,
}

struct AnalyzerState {
    runtime: LspRuntime,
}

fn rust_analyzer_state(binary: PathBuf, root: PathBuf) -> AnalyzerState {
    AnalyzerState {
        runtime: LspRuntime::new(LspRuntimeConfig {
            analyzer_id: "rust-analyzer",
            process_name: "rust-analyzer",
            default_language_id: "rust",
            binary,
            args: Vec::new(),
            mode: LspRuntimeMode::Required,
            fallback_message: "rust-analyzer unavailable.",
            resolver: resolve_rust_analyzer,
            root,
        }),
    }
}

#[allow(dead_code)]
impl AnalyzerState {
    async fn set_root(&self, root: PathBuf) {
        self.runtime.set_root(root).await;
    }

    async fn ensure_document_open(&self, file: &Path) -> Result<()> {
        self.runtime.open_document(file, Some("rust")).await
    }

    async fn sync_changed_file(&self, file: &Path) -> Result<i32> {
        self.runtime.sync_changed_file(file, Some("rust")).await
    }

    pub async fn document_symbols(&self, file: &Path) -> Result<Vec<graph_core::DiscoveredSymbol>> {
        self.runtime.document_symbols(file, Some("rust")).await
    }

    pub async fn prepare_call_hierarchy(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<ra_client::LspCallHierarchyItem>> {
        self.runtime
            .prepare_call_hierarchy(file, line, character, Some("rust"))
            .await
    }

    pub async fn outgoing_calls(
        &self,
        item: &ra_client::LspCallHierarchyItem,
    ) -> Result<Vec<ra_client::LspCallHierarchyOutgoingCall>> {
        self.runtime.outgoing_calls(item).await
    }

    pub async fn incoming_calls(
        &self,
        item: &ra_client::LspCallHierarchyItem,
    ) -> Result<Vec<ra_client::LspCallHierarchyIncomingCall>> {
        self.runtime.incoming_calls(item).await
    }

    pub async fn references(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<ra_client::LspLocation>> {
        self.runtime
            .references(file, line, character, Some("rust"))
            .await
    }

    pub async fn definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .definition(file, line, character, Some("rust"))
            .await
    }

    pub async fn type_definition(
        &self,
        file: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ra_client::LspGotoDefinitionResponse>> {
        self.runtime
            .type_definition(file, line, character, Some("rust"))
            .await
    }

    async fn subscribe_notifications(
        &self,
    ) -> Result<broadcast::Receiver<ra_client::LspNotification>> {
        self.runtime.subscribe_notifications().await
    }
}

#[derive(Debug, Deserialize)]
struct SnapshotQuery {
    mode: Option<GraphMode>,
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RouteTraceQuery {
    method: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct OpenProjectRequest {
    path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenEditorRequest {
    file: PathBuf,
    line: Option<u32>,
    column: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenEditorResponse {
    editor: String,
    file: String,
    line: Option<u32>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsResponse {
    diagnostics_by_file: HashMap<String, Vec<DiagnosticRecord>>,
    diagnostics_by_node: HashMap<String, Vec<DiagnosticRecord>>,
    all_diagnostics: Vec<DiagnosticRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LayoutStore {
    nodes: HashMap<String, LayoutNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutNode {
    node_id: String,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    pinned: Option<bool>,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutNodeInput {
    node_id: String,
    x: f64,
    y: f64,
    vx: Option<f64>,
    vy: Option<f64>,
    pinned: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveLayoutRequest {
    nodes: Vec<LayoutNodeInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveNodeLayoutRequest {
    node: LayoutNodeInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedView {
    id: String,
    name: String,
    filters: serde_json::Value,
    focused_node_id: Option<String>,
    collapsed_groups: Vec<String>,
    layout_overrides: serde_json::Value,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SavedViewsStore {
    views: Vec<SavedView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedViewRequest {
    name: String,
    #[serde(default)]
    filters: serde_json::Value,
    focused_node_id: Option<String>,
    #[serde(default)]
    collapsed_groups: Vec<String>,
    #[serde(default)]
    layout_overrides: serde_json::Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "web_server=info,ra_client=info,project_indexer=info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Serve(args) => serve(args).await,
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    if args.host.is_unspecified() {
        warn!(host = %args.host, "explicitly binding to an unspecified address");
    }

    let project_root = args
        .project
        .clone()
        .unwrap_or(std::env::current_dir().context("failed to read current directory")?)
        .canonicalize()
        .context("failed to canonicalize project root")?;

    let python_analyzer_mode = if args.disable_ty {
        PythonAnalyzerMode::Parser
    } else {
        args.python_analyzer
    };
    let python_ty = Arc::new(PythonTyState::new(
        args.ty_path.clone(),
        python_analyzer_mode,
        project_root.clone(),
    ));
    let typescript_analyzer_mode = if args.disable_typescript_language_server {
        TypeScriptAnalyzerMode::Parser
    } else {
        args.typescript_analyzer
    };
    let typescript_lsp = Arc::new(TypeScriptLspState::new(
        args.typescript_language_server_path.clone(),
        typescript_analyzer_mode,
        project_root.clone(),
    ));
    let qml_analyzer_mode = if args.disable_qmlls {
        QmlAnalyzerMode::Parser
    } else {
        args.qml_analyzer
    };
    let qml_lsp = Arc::new(QmlLspState::new(
        args.qmlls_path.clone(),
        qml_analyzer_mode,
        args.qmlls_build_dir.clone(),
        args.qmlls_no_cmake_calls,
        project_root.clone(),
    ));
    let rust_analyzer = resolve_rust_analyzer(&args.rust_analyzer, &project_root);

    let initial_status = AppStatus {
        app_state: AppState::Empty,
        analyzer_status: AnalyzerStatus::Starting,
        analyzers: initial_analyzer_services(
            AnalyzerStatus::Starting,
            Some(python_ty.status_record()),
            Some(typescript_lsp.status_record()),
            Some(qml_lsp.status_record()),
            0,
            None,
        ),
        python_analyzer: Some(python_ty.status_record()),
        project_name: project_root
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string),
        project_path: Some(project_root.display().to_string()),
        last_updated: None,
        message: None,
        progress: None,
    };
    let initial_snapshot = GraphSnapshot {
        nodes: Vec::new(),
        edges: Vec::new(),
        files: Vec::new(),
        events: Vec::new(),
        status: initial_status.clone(),
    };
    let (ws_tx, _) = broadcast::channel(64);
    let analyzer = Arc::new(rust_analyzer_state(
        rust_analyzer.clone(),
        project_root.clone(),
    ));
    let state = AppStateHandle {
        project_root: Arc::new(RwLock::new(project_root.clone())),
        graph: Arc::new(RwLock::new(initial_snapshot)),
        status: Arc::new(RwLock::new(initial_status)),
        ws_tx,
        analyzer,
        python_ty,
        typescript_lsp,
        qml_lsp,
        diagnostics_by_file: Arc::new(RwLock::new(HashMap::new())),
        diagnostics_by_node: Arc::new(RwLock::new(HashMap::new())),
        watcher: Arc::new(RwLock::new(None)),
        is_indexing: Arc::new(AtomicBool::new(false)),
        enable_editor_open: args.enable_editor_open,
    };
    install_watcher(&state, project_root.clone());

    info!(project_root = %project_root.display(), frontend_dist = %args.frontend_dist.display(), rust_analyzer = %rust_analyzer.display(), python_analyzer = ?python_analyzer_mode, ty = %args.ty_path.display(), typescript_analyzer = ?typescript_analyzer_mode, typescript_language_server = %args.typescript_language_server_path.display(), qml_analyzer = ?qml_analyzer_mode, qmlls = %args.qmlls_path.display(), "starting Rust Code Command Center");

    let index_state = state.clone();
    tokio::spawn(async move {
        analysis::index_and_publish(index_state, project_root).await;
    });

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/status", get(status))
        .route("/api/graph/snapshot", get(snapshot))
        .route("/api/diagnostics", get(diagnostics))
        .route(
            "/api/layout",
            get(layout_get).post(layout_save).delete(layout_clear),
        )
        .route("/api/layout/node", post(layout_save_node))
        .route("/api/views", get(views_get).post(views_create))
        .route("/api/views/{id}", put(views_update).delete(views_delete))
        .route("/api/node/{id}", get(node))
        .route("/api/node/{id}/details", get(node_details))
        .route("/api/trace/node/{id}", get(trace_node))
        .route("/api/trace/edge/{*id}", get(trace_edge))
        .route("/api/trace/route", get(trace_route_query))
        .route("/api/trace/route/by-path", get(trace_route_query))
        .route("/api/trace/route/{*route_key}", get(trace_route))
        .route("/api/context/node/{id}", get(context_node))
        .route("/api/context/edge/{*id}", get(context_edge))
        .route("/api/context/route", get(context_route_query))
        .route("/api/context/trace", post(context_trace))
        .route("/api/search", get(search))
        .route("/api/focus", post(focus))
        .route("/api/editor/open", post(open_in_editor))
        .route("/api/project/open", post(open_project))
        .route("/ws", get(ws_handler))
        .fallback_service(
            ServeDir::new(&args.frontend_dist)
                .not_found_service(ServeFile::new(args.frontend_dist.join("index.html"))),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = TcpListener::bind(SocketAddr::new(args.host, args.port))
        .await
        .with_context(|| format!("failed to bind {}:{}", args.host, args.port))?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{local_addr}");
    println!("{url}");
    info!(%url, "server listening");
    if args.open {
        if let Err(error) = webbrowser::open(&url) {
            warn!(?error, "failed to open browser");
        }
    }

    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: "0.1.0",
    })
}

async fn status(State(state): State<AppStateHandle>) -> Json<AppStatus> {
    Json(state.status.read().clone())
}

async fn snapshot(
    State(state): State<AppStateHandle>,
    Query(query): Query<SnapshotQuery>,
) -> Json<GraphSnapshot> {
    let snapshot = state.graph.read().clone();
    Json(
        query
            .mode
            .map_or(snapshot.clone(), |mode| filter_snapshot(&snapshot, mode)),
    )
}

async fn diagnostics(State(state): State<AppStateHandle>) -> Json<DiagnosticsResponse> {
    let diagnostics_by_file = state.diagnostics_by_file.read().clone();
    let diagnostics_by_node = state.diagnostics_by_node.read().clone();
    let all_diagnostics = diagnostics_by_file
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    Json(DiagnosticsResponse {
        diagnostics_by_file,
        diagnostics_by_node,
        all_diagnostics,
    })
}

async fn layout_get(State(state): State<AppStateHandle>) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    match load_layout(&project_root) {
        Ok(layout) => (StatusCode::OK, Json(layout)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load layout: {error}"),
        )
            .into_response(),
    }
}

async fn layout_save(
    State(state): State<AppStateHandle>,
    Json(request): Json<SaveLayoutRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let updated_at = timestamp();
    let mut layout = LayoutStore::default();
    for node in request.nodes {
        layout.nodes.insert(
            node.node_id.clone(),
            layout_node_from_input(node, updated_at.clone()),
        );
    }
    if let Err(error) = save_layout(&project_root, &layout) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save layout: {error}"),
        )
            .into_response();
    }
    apply_layout_store_to_snapshot(&mut state.graph.write(), &layout);
    (StatusCode::OK, Json(layout)).into_response()
}

async fn layout_save_node(
    State(state): State<AppStateHandle>,
    Json(request): Json<SaveNodeLayoutRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut layout = match load_layout(&project_root) {
        Ok(layout) => layout,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load layout: {error}"),
            )
                .into_response();
        }
    };
    let node = layout_node_from_input(request.node, timestamp());
    layout.nodes.insert(node.node_id.clone(), node.clone());
    if let Err(error) = save_layout(&project_root, &layout) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save layout: {error}"),
        )
            .into_response();
    }
    apply_layout_node_to_snapshot(&mut state.graph.write(), &node);
    (StatusCode::OK, Json(node)).into_response()
}

async fn layout_clear(State(state): State<AppStateHandle>) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    match clear_layout(&project_root) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to clear layout: {error}"),
        )
            .into_response(),
    }
}

async fn views_get(State(state): State<AppStateHandle>) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    match load_views(&project_root) {
        Ok(views) => (StatusCode::OK, Json(views)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to load views: {error}"),
        )
            .into_response(),
    }
}

async fn views_create(
    State(state): State<AppStateHandle>,
    Json(request): Json<SavedViewRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut store = match load_views(&project_root) {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load views: {error}"),
            )
                .into_response();
        }
    };
    let now = timestamp();
    let view = SavedView {
        id: Uuid::new_v4().to_string(),
        name: request.name,
        filters: request.filters,
        focused_node_id: request.focused_node_id,
        collapsed_groups: request.collapsed_groups,
        layout_overrides: request.layout_overrides,
        created_at: now.clone(),
        updated_at: now,
    };
    store.views.push(view.clone());
    if let Err(error) = save_views(&project_root, &store) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save views: {error}"),
        )
            .into_response();
    }
    (StatusCode::CREATED, Json(view)).into_response()
}

async fn views_update(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<SavedViewRequest>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut store = match load_views(&project_root) {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load views: {error}"),
            )
                .into_response();
        }
    };
    let Some(view) = store.views.iter_mut().find(|view| view.id == id) else {
        return (StatusCode::NOT_FOUND, "view not found").into_response();
    };
    view.name = request.name;
    view.filters = request.filters;
    view.focused_node_id = request.focused_node_id;
    view.collapsed_groups = request.collapsed_groups;
    view.layout_overrides = request.layout_overrides;
    view.updated_at = timestamp();
    let response = view.clone();
    if let Err(error) = save_views(&project_root, &store) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save views: {error}"),
        )
            .into_response();
    }
    (StatusCode::OK, Json(response)).into_response()
}

async fn views_delete(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let project_root = state.project_root.read().clone();
    let mut store = match load_views(&project_root) {
        Ok(store) => store,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load views: {error}"),
            )
                .into_response();
        }
    };
    let old_len = store.views.len();
    store.views.retain(|view| view.id != id);
    if old_len == store.views.len() {
        return (StatusCode::NOT_FOUND, "view not found").into_response();
    }
    if let Err(error) = save_views(&project_root, &store) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to save views: {error}"),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read();
    match graph.nodes.iter().find(|node| node.id == id) {
        Some(node) => (StatusCode::OK, Json(node.clone())).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

async fn node_details(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();

    let Some(node) = graph.nodes.iter().find(|node| node.id == id).cloned() else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };

    let node_by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();

    let incoming_edges = graph
        .edges
        .iter()
        .filter(|edge| edge.target == id)
        .cloned()
        .collect::<Vec<_>>();

    let mut references = graph_query::graph_reference_records(&incoming_edges, &node_by_id);

    references.extend(resolve_rust_references(&state, &graph, &node).await);
    references.extend(resolve_python_references(&state, &graph, &node).await);
    references.extend(resolve_typescript_references(&state, &graph, &node).await);
    references.extend(resolve_qml_references(&state, &graph, &node).await);

    graph_query::dedupe_references(&mut references);

    let diagnostics = state
        .diagnostics_by_node
        .read()
        .get(&id)
        .cloned()
        .unwrap_or_default();

    match graph_query::node_details_base(&graph, &id, diagnostics, references) {
        Some(details) => (StatusCode::OK, Json(details)).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

async fn trace_node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let Some(node) = graph.nodes.iter().find(|node| node.id == id) else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    (StatusCode::OK, Json(build_node_trace(&graph, node))).into_response()
}

async fn trace_edge(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let edge_id = id.trim_start_matches('/').to_string();
    let Some(edge) = graph.edges.iter().find(|edge| edge.id == edge_id) else {
        return (StatusCode::NOT_FOUND, "edge not found").into_response();
    };
    (StatusCode::OK, Json(build_edge_trace(&graph, edge))).into_response()
}

async fn trace_route_query(
    State(state): State<AppStateHandle>,
    Query(query): Query<RouteTraceQuery>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let requested = graph_core::route_key(&query.method, &query.path).key;
    match graph_query::find_active_endpoint_by_route_key(&graph, &requested) {
        Some(endpoint) => {
            (StatusCode::OK, Json(build_route_trace(&graph, endpoint))).into_response()
        }
        None => (StatusCode::NOT_FOUND, "active route not found").into_response(),
    }
}

async fn trace_route(
    State(state): State<AppStateHandle>,
    AxumPath(route_key): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let requested = route_key.trim_start_matches('/');
    match graph_query::find_active_endpoint_by_route_key(&graph, requested) {
        Some(endpoint) => {
            (StatusCode::OK, Json(build_route_trace(&graph, endpoint))).into_response()
        }
        None => (StatusCode::NOT_FOUND, "active route not found").into_response(),
    }
}

async fn context_node(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let Some(node) = graph.nodes.iter().find(|node| node.id == id) else {
        return (StatusCode::NOT_FOUND, "node not found").into_response();
    };
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_node_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            node,
        )),
    )
        .into_response()
}

async fn context_edge(
    State(state): State<AppStateHandle>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let edge_id = id.trim_start_matches('/').to_string();
    let Some(edge) = graph.edges.iter().find(|edge| edge.id == edge_id) else {
        return (StatusCode::NOT_FOUND, "edge not found").into_response();
    };
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_edge_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            edge,
        )),
    )
        .into_response()
}

async fn context_route_query(
    State(state): State<AppStateHandle>,
    Query(query): Query<RouteTraceQuery>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let requested = graph_core::route_key(&query.method, &query.path).key;
    let Some(endpoint) = graph_query::find_active_endpoint_by_route_key(&graph, &requested) else {
        return (StatusCode::NOT_FOUND, "active route not found").into_response();
    };
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_route_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            endpoint,
        )),
    )
        .into_response()
}

async fn context_trace(
    State(state): State<AppStateHandle>,
    Json(trace): Json<graph_core::TraceExplanation>,
) -> impl IntoResponse {
    let graph = state.graph.read().clone();
    let diagnostics = state.diagnostics_by_node.read().clone();
    let project_root = state.project_root.read().clone();
    (
        StatusCode::OK,
        Json(build_trace_context_pack(
            &graph,
            &project_root,
            &diagnostics,
            &trace,
        )),
    )
        .into_response()
}

async fn search(
    State(state): State<AppStateHandle>,
    Query(query): Query<SearchQuery>,
) -> Json<SearchResponse> {
    let query = query.q.unwrap_or_default();
    let graph = state.graph.read().clone();

    Json(SearchResponse {
        results: graph_query::search_nodes(&graph, &query, 30),
    })
}

async fn focus(
    State(state): State<AppStateHandle>,
    Json(request): Json<FocusRequest>,
) -> impl IntoResponse {
    let depth = match request.depth {
        FocusDepth::Number(depth) => Some(depth),
        FocusDepth::Full(_) => None,
    };
    let graph = state.graph.read();

    match graph_query::focus_subgraph(&graph, &request.node_id, depth) {
        Some(response) => (StatusCode::OK, Json(response)).into_response(),
        None => (StatusCode::NOT_FOUND, "node not found").into_response(),
    }
}

async fn open_in_editor(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenEditorRequest>,
) -> impl IntoResponse {
    if !state.enable_editor_open {
        return (
            StatusCode::FORBIDDEN,
            "Opening files in an editor is disabled. Restart with --enable-editor-open to enable it.",
        )
            .into_response();
    }
    let root = state.project_root.read().clone();
    let requested_path = if request.file.is_absolute() {
        request.file.clone()
    } else {
        root.join(&request.file)
    };
    let file = match requested_path.canonicalize() {
        Ok(file) => file,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to resolve file path: {error}"),
            )
                .into_response();
        }
    };
    if !file.starts_with(&root) {
        return (
            StatusCode::BAD_REQUEST,
            "refusing to open a file outside the current project",
        )
            .into_response();
    }

    match launch_editor(&file, request.line, request.column.unwrap_or(1)).await {
        Ok(editor) => (
            StatusCode::ACCEPTED,
            Json(OpenEditorResponse {
                editor,
                file: file.display().to_string(),
                line: request.line,
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to open editor: {error}"),
        )
            .into_response(),
    }
}

async fn open_project(
    State(state): State<AppStateHandle>,
    Json(request): Json<OpenProjectRequest>,
) -> impl IntoResponse {
    let root = request
        .path
        .unwrap_or_else(|| state.project_root.read().clone());
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("failed to canonicalize project path: {error}"),
            )
                .into_response();
        }
    };
    *state.project_root.write() = root.clone();
    state.analyzer.set_root(root.clone()).await;
    state.python_ty.set_root(root.clone()).await;
    state.typescript_lsp.set_root(root.clone()).await;
    state.qml_lsp.set_root(root.clone()).await;
    install_watcher(&state, root.clone());
    let index_state = state.clone();
    tokio::spawn(async move {
        analysis::index_and_publish(index_state, root).await;
    });
    (StatusCode::ACCEPTED, Json(state.status.read().clone())).into_response()
}

async fn launch_editor(file: &Path, line: Option<u32>, column: u32) -> Result<String> {
    let mut errors = Vec::new();
    for candidate in editor_candidates(file, line, column) {
        for command_candidate in command_candidates(candidate) {
            let mut command = Command::new(&command_candidate.program);
            command.args(&command_candidate.args);
            match command.spawn() {
                Ok(_child) => return Ok(command_candidate.label),
                Err(error) => errors.push(format!("{}: {error}", command_candidate.label)),
            }
        }
    }
    anyhow::bail!(
        "no editor command worked. Set RUST_WATCHER_EDITOR, RUST_WATCHER_TERMINAL, VISUAL, or EDITOR. Tried: {}",
        errors.join("; ")
    )
}

struct EditorCommand {
    label: String,
    program: String,
    args: Vec<String>,
}

fn command_candidates(candidate: EditorCommand) -> Vec<EditorCommand> {
    if is_terminal_editor(&candidate.program) {
        return terminal_wrapped_editor_candidates(&candidate);
    }
    vec![candidate]
}

fn is_terminal_editor(program: &str) -> bool {
    matches!(
        Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program),
        "nvim" | "vim" | "vi" | "hx" | "helix" | "kak" | "micro" | "nano"
    )
}

fn terminal_wrapped_editor_candidates(editor: &EditorCommand) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_TERMINAL", "TERMINAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if let Some(candidate) = terminal_command_from_env(env_name, &command, editor) {
                candidates.push(candidate);
            }
        }
    }
    for terminal in [
        "x-terminal-emulator",
        "kitty",
        "alacritty",
        "wezterm",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "xterm",
    ] {
        candidates.push(wrap_editor_in_terminal(
            terminal.to_string(),
            Vec::new(),
            editor,
        ));
    }
    candidates
}

fn terminal_command_from_env(
    env_name: &str,
    command: &str,
    editor: &EditorCommand,
) -> Option<EditorCommand> {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }
    let program = parts.remove(0);
    let mut candidate = wrap_editor_in_terminal(program, parts, editor);
    candidate.label = format!("{env_name}: {}", candidate.label);
    Some(candidate)
}

fn wrap_editor_in_terminal(
    terminal: String,
    mut terminal_args: Vec<String>,
    editor: &EditorCommand,
) -> EditorCommand {
    let terminal_name = Path::new(&terminal)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&terminal);
    let editor_program = editor.program.clone();
    let editor_args = editor.args.clone();

    match terminal_name {
        "wezterm" => {
            terminal_args.push("start".into());
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "gnome-terminal" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        "xfce4-terminal" => {
            terminal_args.push("--disable-server".into());
            terminal_args.push("--command".into());
            terminal_args.push(shell_join(&editor.program, &editor.args));
        }
        "kitty" => {
            terminal_args.push("--".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
        _ => {
            terminal_args.push("-e".into());
            terminal_args.push(editor_program);
            terminal_args.extend(editor_args);
        }
    }

    EditorCommand {
        label: format!("{terminal_name} -> {}", editor.label),
        program: terminal,
        args: terminal_args,
    }
}

fn shell_join(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .map(|part| shell_quote(&part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:+".contains(ch))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn editor_candidates(file: &Path, line: Option<u32>, column: u32) -> Vec<EditorCommand> {
    let mut candidates = Vec::new();
    for env_name in ["RUST_WATCHER_EDITOR", "VISUAL"] {
        if let Ok(command) = std::env::var(env_name) {
            if !command.trim().is_empty() {
                candidates.push(editor_command_from_env(
                    env_name, &command, file, line, column,
                ));
            }
        }
    }

    let file_text = file.display().to_string();
    let line_col = match line {
        Some(line) => format!("{file_text}:{line}:{column}"),
        None => file_text.clone(),
    };
    let line_only = match line {
        Some(line) => format!("{file_text}:{line}"),
        None => file_text.clone(),
    };
    for program in ["code", "codium", "code-insiders"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: if line.is_some() {
                vec!["--goto".into(), line_col.clone()]
            } else {
                vec![file_text.clone()]
            },
        });
    }
    candidates.push(EditorCommand {
        label: "zed".into(),
        program: "zed".into(),
        args: vec![line_only],
    });
    for program in ["rustrover", "idea"] {
        candidates.push(EditorCommand {
            label: program.to_string(),
            program: program.to_string(),
            args: match line {
                Some(line) => vec!["--line".into(), line.to_string(), file_text.clone()],
                None => vec![file_text.clone()],
            },
        });
    }
    candidates.push(EditorCommand {
        label: "xdg-open".into(),
        program: "xdg-open".into(),
        args: vec![file_text],
    });
    if let Ok(command) = std::env::var("EDITOR") {
        if !command.trim().is_empty() {
            candidates.push(editor_command_from_env(
                "EDITOR", &command, file, line, column,
            ));
        }
    }
    candidates
}

fn editor_command_from_env(
    env_name: &str,
    command: &str,
    file: &Path,
    line: Option<u32>,
    column: u32,
) -> EditorCommand {
    let mut parts = command
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let program = parts
        .first()
        .cloned()
        .unwrap_or_else(|| command.to_string());
    let file_text = file.display().to_string();
    let line_text = line.unwrap_or(1).to_string();
    let column_text = column.to_string();
    let had_template = parts.iter().any(|part| {
        part.contains("{file}") || part.contains("{line}") || part.contains("{column}")
    });
    if !parts.is_empty() {
        parts.remove(0);
    }
    let mut args = parts
        .into_iter()
        .map(|part| {
            part.replace("{file}", &file_text)
                .replace("{line}", &line_text)
                .replace("{column}", &column_text)
        })
        .collect::<Vec<_>>();
    if !had_template {
        args.extend(default_editor_args(
            &program, &args, &file_text, line, column,
        ));
    }
    EditorCommand {
        label: format!("{env_name}={program}"),
        program,
        args,
    }
}

fn default_editor_args(
    program: &str,
    existing_args: &[String],
    file: &str,
    line: Option<u32>,
    column: u32,
) -> Vec<String> {
    let program_name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    match (program_name, line) {
        ("code" | "codium" | "code-insiders", Some(line)) => {
            if existing_args.iter().any(|arg| arg == "--goto") {
                vec![format!("{file}:{line}:{column}")]
            } else {
                vec!["--goto".into(), format!("{file}:{line}:{column}")]
            }
        }
        ("zed", Some(line)) => vec![format!("{file}:{line}")],
        ("idea" | "rustrover", Some(line)) => vec!["--line".into(), line.to_string(), file.into()],
        ("nvim" | "vim" | "vi" | "kak", Some(line)) => {
            vec![format!("+{line}"), file.into()]
        }
        ("nano" | "micro", Some(line)) => vec![format!("+{line},{column}"), file.into()],
        ("hx" | "helix", Some(line)) => vec![format!("{file}:{line}:{column}")],
        _ => vec![file.into()],
    }
}

fn install_watcher(state: &AppStateHandle, root: PathBuf) {
    let handle = tokio::runtime::Handle::current();
    let watch_state = state.clone();
    match start_watcher(root.clone(), move |event| {
        let state = watch_state.clone();
        if state.is_indexing.load(Ordering::Relaxed) {
            return;
        }
        let changed_path = event
            .paths
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let root = state.project_root.read().clone();
        handle.spawn(async move {
            let analysis_event = analysis_event(
                AnalysisEventType::Analyzer,
                format!("File changed: {changed_path}"),
                Some(changed_path),
            );
            {
                let mut graph = state.graph.write();
                graph.events.push(analysis_event.clone());
            }
            update_status(&state, |status| {
                status.analyzer_status = AnalyzerStatus::Stale;
                status.message = Some("File changed. Re-indexing workspace.".into());
                status.progress = Some(0);
            });
            let _ = state
                .ws_tx
                .send(ServerMessage::AnalysisEvent(analysis_event));
            sleep(Duration::from_millis(250)).await;
            let changed_files = event
                .paths
                .iter()
                .map(|path| project_indexer::relative_to(&root, path))
                .collect::<Vec<_>>();
            analysis::index_and_patch(state, root, changed_files).await;
        });
    }) {
        Ok(watcher) => {
            *state.watcher.write() = Some(watcher);
            info!(project_root = %root.display(), "file watcher installed");
        }
        Err(error) => {
            warn!(project_root = %root.display(), ?error, "failed to install file watcher")
        }
    }
}

async fn ws_handler(
    State(state): State<AppStateHandle>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, state))
}

async fn websocket(socket: WebSocket, state: AppStateHandle) {
    info!("websocket connected");
    let (mut sender, mut receiver) = socket.split();
    let initial = ServerMessage::GraphSnapshot(state.graph.read().clone());
    if let Ok(text) = serde_json::to_string(&initial) {
        let _ = sender.send(Message::Text(text.into())).await;
    }

    let mut rx = state.ws_tx.subscribe();
    let forward = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            match serde_json::to_string(&message) {
                Ok(text) => {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(error) => warn!(?error, "failed to serialize websocket message"),
            }
        }
    });

    while let Some(message) = receiver.next().await {
        if matches!(message, Ok(Message::Close(_)) | Err(_)) {
            break;
        }
    }
    forward.abort();
    info!("websocket disconnected");
}

fn apply_lsp_diagnostics(
    state: &AppStateHandle,
    language: Option<LanguageId>,
    source_override: Option<&str>,
    params: ra_client::LspPublishDiagnosticsParams,
) {
    let Some(path) = Url::parse(params.uri.as_str())
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
    else {
        return;
    };
    let root = state.project_root.read().clone();
    let file = project_indexer::relative_to(&root, &path);
    let graph = state.graph.read().clone();
    let symbol_index = SymbolIndex::from_nodes(&graph.nodes);
    let language = language.unwrap_or_else(|| language_for_path(&file));
    let diagnostics = params
        .diagnostics
        .into_iter()
        .enumerate()
        .map(|(idx, diagnostic)| {
            diagnostic_from_lsp_with_language(
                language.clone(),
                &file,
                idx,
                diagnostic,
                &symbol_index,
                source_override,
            )
        })
        .collect::<Vec<_>>();

    state
        .diagnostics_by_file
        .write()
        .insert(file.clone(), diagnostics.clone());
    rebuild_diagnostics_by_node(state);
    update_project_file_diagnostics(state, &file, diagnostics.len() as u32);

    let _ = state.ws_tx.send(ServerMessage::GraphPatch(GraphPatch {
        diagnostics,
        changed_files: vec![file],
        ..GraphPatch::default()
    }));
}

#[cfg(test)]
fn diagnostic_from_lsp(
    file: &str,
    index: usize,
    diagnostic: ra_client::LspDiagnostic,
    symbol_index: &SymbolIndex,
) -> DiagnosticRecord {
    diagnostic_from_lsp_with_language(
        LanguageId::Rust,
        file,
        index,
        diagnostic,
        symbol_index,
        None,
    )
}

fn diagnostic_from_lsp_with_language(
    language: LanguageId,
    file: &str,
    index: usize,
    diagnostic: ra_client::LspDiagnostic,
    symbol_index: &SymbolIndex,
    source_override: Option<&str>,
) -> DiagnosticRecord {
    let range = graph_core::TextRange {
        start: graph_core::TextPosition {
            line: diagnostic.range.start.line,
            character: diagnostic.range.start.character,
        },
        end: graph_core::TextPosition {
            line: diagnostic.range.end.line,
            character: diagnostic.range.end.character,
        },
    };
    let related_node_ids = related_nodes_for_range(symbol_index, file, range);
    let code = diagnostic.code.map(|code| match code {
        ra_client::LspNumberOrString::Number(value) => value.to_string(),
        ra_client::LspNumberOrString::String(value) => value,
    });
    DiagnosticRecord {
        id: format!(
            "diagnostic:{file}:{}:{}:{index}",
            range.start.line, range.start.character
        ),
        language,
        file: file.to_string(),
        range: Some(range),
        severity: diagnostic_severity(diagnostic.severity),
        source: source_override.map(str::to_string).or(diagnostic.source),
        message: diagnostic.message,
        code,
        related_node_ids,
    }
}

fn diagnostic_severity(severity: Option<ra_client::LspDiagnosticSeverity>) -> DiagnosticSeverity {
    match severity {
        Some(ra_client::LspDiagnosticSeverity::ERROR) => DiagnosticSeverity::Error,
        Some(ra_client::LspDiagnosticSeverity::WARNING) => DiagnosticSeverity::Warning,
        Some(ra_client::LspDiagnosticSeverity::INFORMATION) => DiagnosticSeverity::Information,
        Some(ra_client::LspDiagnosticSeverity::HINT) => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Information,
    }
}

fn related_nodes_for_range(
    symbol_index: &SymbolIndex,
    file: &str,
    range: graph_core::TextRange,
) -> Vec<String> {
    symbol_index
        .find_by_file(file)
        .into_iter()
        .filter(|symbol| ranges_overlap(symbol.range, range))
        .map(|symbol| symbol.node_id.clone())
        .collect()
}

fn ranges_overlap(left: graph_core::TextRange, right: graph_core::TextRange) -> bool {
    position_le(left.start, right.end) && position_le(right.start, left.end)
}

fn position_le(left: graph_core::TextPosition, right: graph_core::TextPosition) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
}

fn rebuild_diagnostics_by_node(state: &AppStateHandle) {
    let mut by_node: HashMap<String, Vec<DiagnosticRecord>> = HashMap::new();
    for diagnostic in state.diagnostics_by_file.read().values().flatten() {
        for node_id in &diagnostic.related_node_ids {
            by_node
                .entry(node_id.clone())
                .or_default()
                .push(diagnostic.clone());
        }
    }
    *state.diagnostics_by_node.write() = by_node;
}

fn update_project_file_diagnostics(state: &AppStateHandle, file: &str, count: u32) {
    let mut graph = state.graph.write();
    if let Some(project_file) = graph
        .files
        .iter_mut()
        .find(|project_file| project_file.path == file)
    {
        project_file.diagnostics_count = count;
    }
}

async fn resolve_rust_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if node.language.as_deref() != Some(LanguageId::Rust.as_str())
        || !matches!(
            node.node_type,
            graph_core::NodeType::Function | graph_core::NodeType::Method
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let locations = match timeout(
        Duration::from_secs(4),
        state.analyzer.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "rust-analyzer references failed");
            return Vec::new();
        }
        Err(_) => {
            warn!(node = %node.id, "rust-analyzer references timed out");
            return Vec::new();
        }
    };

    references_from_locations(graph, &project_root, locations)
}

async fn resolve_python_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if state.python_ty.is_parser_only()
        || node.language.as_deref() != Some(LanguageId::Python.as_str())
        || !matches!(
            node.node_type,
            graph_core::NodeType::Function
                | graph_core::NodeType::Method
                | graph_core::NodeType::Class
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let locations = match timeout(
        Duration::from_secs(4),
        state.python_ty.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "ty references failed");
            return Vec::new();
        }
        Err(_) => {
            warn!(node = %node.id, "ty references timed out");
            return Vec::new();
        }
    };

    references_from_locations(graph, &project_root, locations)
}

async fn resolve_typescript_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if state.typescript_lsp.is_parser_only()
        || !matches!(
            node.language.as_deref(),
            Some("typescript" | "javascript")
                | Some("TypeScript")
                | Some("JavaScript")
                | Some("ts")
                | Some("js")
        )
        || !matches!(
            node.node_type,
            graph_core::NodeType::Function
                | graph_core::NodeType::Method
                | graph_core::NodeType::Class
                | graph_core::NodeType::Interface
                | graph_core::NodeType::TypeAlias
                | graph_core::NodeType::Component
                | graph_core::NodeType::Hook
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let mut locations = match timeout(
        Duration::from_secs(4),
        state.typescript_lsp.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "typescript-language-server references failed");
            Vec::new()
        }
        Err(_) => {
            warn!(node = %node.id, "typescript-language-server references timed out");
            Vec::new()
        }
    };

    if let Ok(Ok(response)) = timeout(
        Duration::from_secs(3),
        state.typescript_lsp.definition(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        locations.extend(locations_from_definition_response(response));
    }
    if let Ok(Ok(response)) = timeout(
        Duration::from_secs(3),
        state.typescript_lsp.type_definition(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        locations.extend(locations_from_definition_response(response));
    }

    references_from_locations(graph, &project_root, locations)
}

async fn resolve_qml_references(
    state: &AppStateHandle,
    graph: &GraphSnapshot,
    node: &GraphNode,
) -> Vec<ReferenceRecord> {
    if state.qml_lsp.is_parser_only()
        || node.language.as_deref() != Some(LanguageId::Qml.as_str())
        || !matches!(
            node.node_type,
            graph_core::NodeType::Object
                | graph_core::NodeType::Property
                | graph_core::NodeType::Signal
                | graph_core::NodeType::Handler
                | graph_core::NodeType::Function
                | graph_core::NodeType::Component
                | graph_core::NodeType::File
        )
    {
        return Vec::new();
    }
    let Some(file) = node.file.as_ref() else {
        return Vec::new();
    };
    let Some(selection_range) = node.selection_range.or(node.range) else {
        return Vec::new();
    };
    let project_root = state.project_root.read().clone();
    let absolute_file = project_root.join(file);
    let mut locations = match timeout(
        Duration::from_secs(4),
        state.qml_lsp.references(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        Ok(Ok(locations)) => locations,
        Ok(Err(error)) => {
            warn!(?error, node = %node.id, "qmlls references failed");
            Vec::new()
        }
        Err(_) => {
            warn!(node = %node.id, "qmlls references timed out");
            Vec::new()
        }
    };

    if let Ok(Ok(response)) = timeout(
        Duration::from_secs(3),
        state.qml_lsp.definition(
            &absolute_file,
            selection_range.start.line,
            selection_range.start.character,
        ),
    )
    .await
    {
        locations.extend(locations_from_definition_response(response));
    }

    references_from_locations(graph, &project_root, locations)
}

fn references_from_locations(
    graph: &GraphSnapshot,
    project_root: &Path,
    locations: Vec<ra_client::LspLocation>,
) -> Vec<ReferenceRecord> {
    let symbol_index = SymbolIndex::from_nodes(&graph.nodes);
    let node_by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    locations
        .into_iter()
        .filter_map(|location| {
            reference_from_location(project_root, &symbol_index, &node_by_id, location)
        })
        .collect()
}

fn reference_from_location(
    project_root: &Path,
    symbol_index: &SymbolIndex,
    node_by_id: &HashMap<&str, &GraphNode>,
    location: ra_client::LspLocation,
) -> Option<ReferenceRecord> {
    let path = Url::parse(location.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file = project_indexer::relative_to(project_root, &path);
    let range = graph_core::TextRange {
        start: graph_core::TextPosition {
            line: location.range.start.line,
            character: location.range.start.character,
        },
        end: graph_core::TextPosition {
            line: location.range.end.line,
            character: location.range.end.character,
        },
    };
    let node = symbol_index
        .find_by_uri_path_position(&path, range.start.line, range.start.character)
        .and_then(|symbol| node_by_id.get(symbol.node_id.as_str()).copied())
        .cloned();
    Some(ReferenceRecord {
        node,
        location: SourceLocation {
            file,
            line: range.start.line + 1,
            character: range.start.character,
            range: Some(range),
        },
    })
}

fn decorate_app_status(state: &AppStateHandle, status: &mut AppStatus) {
    let snapshot = state.graph.read().clone();
    decorate_app_status_for_snapshot(state, status, &snapshot);
}

fn decorate_app_status_for_snapshot(
    state: &AppStateHandle,
    status: &mut AppStatus,
    snapshot: &GraphSnapshot,
) {
    let python = state.python_ty.status_record();
    status.python_analyzer = Some(python.clone());
    status.analyzers = analyzer_services_from_snapshot(
        status.analyzer_status,
        status.message.clone(),
        Some(python),
        Some(state.typescript_lsp.status_record()),
        Some(state.qml_lsp.status_record()),
        snapshot,
        status.last_updated.clone(),
    );
}

fn initial_analyzer_services(
    rust_status: AnalyzerStatus,
    python: Option<PythonAnalyzerStatus>,
    typescript: Option<TypeScriptAnalyzerStatus>,
    qml_status: Option<QmlAnalyzerStatus>,
    files_indexed: u32,
    last_updated: Option<String>,
) -> Vec<AnalyzerServiceStatus> {
    analyzer_services_from_counts(
        rust_status,
        None,
        python,
        typescript,
        qml_status,
        AnalyzerFileCounts {
            rust: files_indexed,
            typescript: 0,
            python: 0,
            qml: 0,
        },
        last_updated,
    )
}

#[derive(Debug, Clone, Copy, Default)]
struct AnalyzerFileCounts {
    rust: u32,
    typescript: u32,
    python: u32,
    qml: u32,
}

fn analyzer_services_from_snapshot(
    rust_status: AnalyzerStatus,
    rust_message: Option<String>,
    python: Option<PythonAnalyzerStatus>,
    typescript: Option<TypeScriptAnalyzerStatus>,
    qml_status: Option<QmlAnalyzerStatus>,
    snapshot: &GraphSnapshot,
    last_updated: Option<String>,
) -> Vec<AnalyzerServiceStatus> {
    let mut counts = AnalyzerFileCounts::default();
    for file in &snapshot.files {
        if file.path.ends_with(".rs") {
            counts.rust += 1;
        } else if typescript::is_typescript_path(&file.path) {
            counts.typescript += 1;
        } else if python::is_python_path(&file.path) {
            counts.python += 1;
        } else if qml::is_qml_path(&file.path) {
            counts.qml += 1;
        }
    }
    analyzer_services_from_counts(
        rust_status,
        rust_message,
        python,
        typescript,
        qml_status,
        counts,
        last_updated,
    )
}

fn analyzer_services_from_counts(
    rust_status: AnalyzerStatus,
    rust_message: Option<String>,
    python: Option<PythonAnalyzerStatus>,
    typescript: Option<TypeScriptAnalyzerStatus>,
    qml_status: Option<QmlAnalyzerStatus>,
    counts: AnalyzerFileCounts,
    last_updated: Option<String>,
) -> Vec<AnalyzerServiceStatus> {
    let mut services = vec![AnalyzerServiceStatus {
        id: "rust-analyzer".into(),
        kind: AnalyzerKind::Rust,
        engine: AnalyzerEngine::RustAnalyzer,
        label: "rust-analyzer".into(),
        status: rust_status,
        mode: None,
        message: rust_message,
        capabilities: vec![
            AnalyzerCapability::Symbols,
            AnalyzerCapability::Diagnostics,
            AnalyzerCapability::References,
            AnalyzerCapability::Definitions,
            AnalyzerCapability::TypeDefinitions,
            AnalyzerCapability::CallHierarchy,
            AnalyzerCapability::SemanticCalls,
        ],
        files_indexed: counts.rust,
        last_updated: last_updated.clone(),
        provider: AnalyzerProvider::Local,
        billable: false,
        credits_used: None,
    }];

    if counts.python > 0 {
        if let Some(python) = python {
            let ty_status = analyzer_status_from_python_status(&python.status);
            let ty_ready = ty_status == AnalyzerStatus::Ready;
            let ty_unavailable_auto = python.mode == "auto"
                && matches!(ty_status, AnalyzerStatus::Fallback | AnalyzerStatus::Error);
            if python.mode == "ty" || ty_ready || ty_unavailable_auto {
                services.push(AnalyzerServiceStatus {
                    id: "python-ty".into(),
                    kind: AnalyzerKind::Python,
                    engine: AnalyzerEngine::Ty,
                    label: "ty".into(),
                    status: ty_status,
                    mode: Some(python.mode.clone()),
                    message: python.message.clone(),
                    capabilities: if ty_ready {
                        vec![
                            AnalyzerCapability::Symbols,
                            AnalyzerCapability::Diagnostics,
                            AnalyzerCapability::References,
                            AnalyzerCapability::Definitions,
                            AnalyzerCapability::TypeDefinitions,
                            AnalyzerCapability::CallHierarchy,
                            AnalyzerCapability::SemanticCalls,
                        ]
                    } else {
                        Vec::new()
                    },
                    files_indexed: counts.python,
                    last_updated: last_updated.clone(),
                    provider: AnalyzerProvider::Local,
                    billable: false,
                    credits_used: None,
                });
            }
            if python.mode == "parser" || ty_unavailable_auto || python.status == "parser only" {
                services.push(AnalyzerServiceStatus {
                    id: "python-parser".into(),
                    kind: AnalyzerKind::Python,
                    engine: AnalyzerEngine::Parser,
                    label: "Python parser".into(),
                    status: AnalyzerStatus::Ready,
                    mode: Some("parser".into()),
                    message: if ty_unavailable_auto {
                        Some(
                            "ty not found, parser fallback active. Install with: uv tool install ty"
                                .into(),
                        )
                    } else {
                        None
                    },
                    capabilities: vec![AnalyzerCapability::Symbols],
                    files_indexed: counts.python,
                    last_updated: last_updated.clone(),
                    provider: AnalyzerProvider::Local,
                    billable: false,
                    credits_used: None,
                });
            }
        }
    }

    if counts.typescript > 0 {
        let typescript = typescript.unwrap_or(TypeScriptAnalyzerStatus {
            mode: "parser".into(),
            status: "parser only".into(),
            message: None,
        });
        let ts_status = status_to_analyzer_status(&typescript.status);
        let ts_ready = ts_status == AnalyzerStatus::Ready;
        let ts_unavailable_auto = typescript.mode == "auto"
            && matches!(ts_status, AnalyzerStatus::Fallback | AnalyzerStatus::Error);
        if typescript.mode == "typescript-language-server" || ts_ready || ts_unavailable_auto {
            services.push(AnalyzerServiceStatus {
                id: "typescript-language-server".into(),
                kind: AnalyzerKind::TypeScript,
                engine: AnalyzerEngine::TypeScriptLanguageServer,
                label: "TypeScript language server".into(),
                status: ts_status,
                mode: Some(typescript.mode.clone()),
                message: typescript.message.clone(),
                capabilities: if ts_ready {
                    vec![
                        AnalyzerCapability::Symbols,
                        AnalyzerCapability::Diagnostics,
                        AnalyzerCapability::References,
                        AnalyzerCapability::Definitions,
                        AnalyzerCapability::TypeDefinitions,
                    ]
                } else {
                    Vec::new()
                },
                files_indexed: counts.typescript,
                last_updated: last_updated.clone(),
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
        if typescript.mode == "parser" || ts_unavailable_auto || typescript.status == "parser only"
        {
            services.push(AnalyzerServiceStatus {
                id: "typescript-parser".into(),
                kind: AnalyzerKind::TypeScript,
                engine: AnalyzerEngine::TypeScriptParser,
                label: "TypeScript parser".into(),
                status: AnalyzerStatus::Ready,
                mode: Some("parser".into()),
                message: if ts_unavailable_auto {
                    Some(
                        "Not installed, parser fallback active. Install with: cd frontend && pnpm add -D typescript typescript-language-server".into(),
                    )
                } else {
                    None
                },
                capabilities: vec![AnalyzerCapability::Symbols],
                files_indexed: counts.typescript,
                last_updated: last_updated.clone(),
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
    }
    if counts.qml > 0 {
        let qml_status = qml_status.unwrap_or(QmlAnalyzerStatus {
            mode: "parser".into(),
            status: "parser only".into(),
            message: None,
        });
        let qmlls_status = qml_status_to_analyzer_status(&qml_status.status);
        let qmlls_ready = qmlls_status == AnalyzerStatus::Ready;
        let qmlls_unavailable_auto = qml_status.mode == "auto"
            && matches!(
                qmlls_status,
                AnalyzerStatus::Fallback | AnalyzerStatus::Error
            );
        if qml_status.mode == "qmlls" || qmlls_ready || qmlls_unavailable_auto {
            services.push(AnalyzerServiceStatus {
                id: "qmlls".into(),
                kind: AnalyzerKind::Qml,
                engine: AnalyzerEngine::QmlLanguageServer,
                label: "qmlls".into(),
                status: qmlls_status,
                mode: Some(qml_status.mode.clone()),
                message: qml_status.message.clone(),
                capabilities: if qmlls_ready {
                    vec![
                        AnalyzerCapability::Symbols,
                        AnalyzerCapability::Diagnostics,
                        AnalyzerCapability::References,
                        AnalyzerCapability::Definitions,
                    ]
                } else {
                    Vec::new()
                },
                files_indexed: counts.qml,
                last_updated: last_updated.clone(),
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
        if qml_status.mode == "parser"
            || qmlls_unavailable_auto
            || qml_status.status == "parser only"
        {
            services.push(AnalyzerServiceStatus {
                id: "qml-parser".into(),
                kind: AnalyzerKind::Qml,
                engine: AnalyzerEngine::QmlParser,
                label: "QML parser".into(),
                status: if qmlls_unavailable_auto {
                    AnalyzerStatus::Fallback
                } else {
                    AnalyzerStatus::Ready
                },
                mode: Some("parser".into()),
                message: if qmlls_unavailable_auto {
                    Some("qmlls not found, parser fallback active. Install Qt/qmlls or pass --qmlls-path.".into())
                } else {
                    None
                },
                capabilities: vec![AnalyzerCapability::Symbols],
                files_indexed: counts.qml,
                last_updated,
                provider: AnalyzerProvider::Local,
                billable: false,
                credits_used: None,
            });
        }
    }
    services
}

fn analyzer_status_from_python_status(status: &str) -> AnalyzerStatus {
    let status = status.to_ascii_lowercase();
    if status.contains("ready") {
        AnalyzerStatus::Ready
    } else if status.contains("restart") || status.contains("starting") {
        AnalyzerStatus::Starting
    } else if status.contains("error") {
        AnalyzerStatus::Error
    } else if status.contains("unavailable") || status.contains("parser only") {
        AnalyzerStatus::Fallback
    } else {
        AnalyzerStatus::Stale
    }
}

fn update_status<F>(state: &AppStateHandle, mut update: F)
where
    F: FnMut(&mut AppStatus),
{
    let mut status = state.status.read().clone();
    update(&mut status);
    decorate_app_status(state, &mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    state.graph.write().status = status.clone();
    let _ = state.ws_tx.send(ServerMessage::AnalyzerStatus(status));
}

fn publish_snapshot(state: &AppStateHandle, mut snapshot: GraphSnapshot) {
    let project_root = state.project_root.read().clone();
    apply_saved_layout(&mut snapshot, &project_root);
    let python = state.python_ty.status_record();
    let typescript = state.typescript_lsp.status_record();
    let qml_status = state.qml_lsp.status_record();
    snapshot.status.python_analyzer = Some(python.clone());
    snapshot.status.analyzers = analyzer_services_from_snapshot(
        snapshot.status.analyzer_status,
        snapshot.status.message.clone(),
        Some(python),
        Some(typescript),
        Some(qml_status),
        &snapshot,
        snapshot.status.last_updated.clone(),
    );
    snapshot.status.last_updated = Some(timestamp());
    *state.status.write() = snapshot.status.clone();
    *state.graph.write() = snapshot.clone();
    let _ = state.ws_tx.send(ServerMessage::GraphSnapshot(snapshot));
}

fn ready_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Ready;
    status.message = Some(message.into());
    status.progress = Some(100);
    decorate_app_status(state, &mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

fn fallback_status(state: &AppStateHandle, message: &str) -> AppStatus {
    let mut status = state.status.read().clone();
    status.app_state = AppState::Normal;
    status.analyzer_status = AnalyzerStatus::Fallback;
    status.message = Some(message.into());
    status.progress = Some(100);
    decorate_app_status(state, &mut status);
    status.last_updated = Some(timestamp());
    *state.status.write() = status.clone();
    let _ = state
        .ws_tx
        .send(ServerMessage::AnalyzerStatus(status.clone()));
    status
}

fn analysis_event(
    event_type: AnalysisEventType,
    message: impl Into<String>,
    file: Option<String>,
) -> AnalysisEvent {
    AnalysisEvent {
        id: Uuid::new_v4().to_string(),
        event_type,
        message: message.into(),
        timestamp: timestamp(),
        file,
    }
}

fn layout_node_from_input(input: LayoutNodeInput, updated_at: String) -> LayoutNode {
    LayoutNode {
        node_id: input.node_id,
        x: input.x,
        y: input.y,
        vx: input.vx.unwrap_or_default(),
        vy: input.vy.unwrap_or_default(),
        pinned: input.pinned,
        updated_at,
    }
}

fn storage_dir_for_project(project_root: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    project_root.display().to_string().hash(&mut hasher);
    let project_hash = format!("{:016x}", hasher.finish());
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("rust-watcher").join(project_hash)
}

fn layout_path(project_root: &Path) -> PathBuf {
    storage_dir_for_project(project_root).join("layout.json")
}

fn views_path(project_root: &Path) -> PathBuf {
    storage_dir_for_project(project_root).join("views.json")
}

fn load_layout(project_root: &Path) -> Result<LayoutStore> {
    let path = layout_path(project_root);
    if !path.exists() {
        return Ok(LayoutStore::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    match serde_json::from_str(&text) {
        Ok(layout) => Ok(layout),
        Err(error) => {
            let corrupt_path = path.with_file_name(format!(
                "layout.corrupt.{}.json",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_millis())
                    .unwrap_or_default()
            ));
            let _ = std::fs::rename(&path, &corrupt_path);
            warn!(
                error = %error,
                layout = %path.display(),
                backup = %corrupt_path.display(),
                "corrupt layout cache renamed; starting fresh"
            );
            Ok(LayoutStore::default())
        }
    }
}

fn save_layout(project_root: &Path, layout: &LayoutStore) -> Result<()> {
    let path = layout_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(layout).context("failed to serialize layout")?;
    std::fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
}

fn clear_layout(project_root: &Path) -> Result<()> {
    let path = layout_path(project_root);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn apply_saved_layout(snapshot: &mut GraphSnapshot, project_root: &Path) {
    match load_layout(project_root) {
        Ok(layout) => apply_layout_store_to_snapshot(snapshot, &layout),
        Err(error) => warn!(?error, "failed to load saved layout"),
    }
}

fn apply_layout_store_to_snapshot(snapshot: &mut GraphSnapshot, layout: &LayoutStore) {
    for node in &mut snapshot.nodes {
        if let Some(layout_node) = layout.nodes.get(&node.id) {
            apply_layout_node(node, layout_node);
        }
    }
}

fn apply_layout_node_to_snapshot(snapshot: &mut GraphSnapshot, layout_node: &LayoutNode) {
    if let Some(node) = snapshot
        .nodes
        .iter_mut()
        .find(|node| node.id == layout_node.node_id)
    {
        apply_layout_node(node, layout_node);
    }
}

fn apply_layout_node(node: &mut GraphNode, layout_node: &LayoutNode) {
    node.x = layout_node.x;
    node.y = layout_node.y;
    node.vx = layout_node.vx;
    node.vy = layout_node.vy;
    if layout_node.pinned.is_some() {
        node.pinned = layout_node.pinned;
    }
}

fn load_views(project_root: &Path) -> Result<SavedViewsStore> {
    let path = views_path(project_root);
    if !path.exists() {
        return Ok(SavedViewsStore::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_views(project_root: &Path, views: &SavedViewsStore) -> Result<()> {
    let path = views_path(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(views).context("failed to serialize views")?;
    std::fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))
}

fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::{
        EdgeConfidence, EdgeType, LspPosition, LspRange, SourceReachability, TraceStepKind,
        Visibility,
    };

    fn test_node(label: &str, file: Option<&str>, module: Option<&str>) -> GraphNode {
        let range = LspRange {
            start: LspPosition {
                line: 0,
                character: 0,
            },
            end: LspPosition {
                line: 0,
                character: label.len() as u32,
            },
        };
        GraphNode {
            id: format!("fn:{}@1", label),
            language: Some("rust".into()),
            node_type: graph_core::NodeType::Function,
            label: label.into(),
            file: file.map(str::to_string),
            module: module.map(str::to_string),
            crate_name: Some("demo".into()),
            line: Some(1),
            visibility: Some(Visibility::Pub),
            is_async: None,
            is_unsafe: None,
            is_generic: None,
            signature: None,
            description: None,
            pinned: None,
            bookmarked: None,
            connections: None,
            range: Some(range),
            selection_range: Some(range),
            reachability: None,
            reachable_from: None,
            detached_reason: None,
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
        }
    }

    fn test_edge(
        edge_type: EdgeType,
        source: impl Into<String>,
        target: impl Into<String>,
        confidence: EdgeConfidence,
    ) -> graph_core::GraphEdge {
        let source = source.into();
        let target = target.into();
        graph_core::GraphEdge {
            id: graph_core::edge_id(edge_type, &source, &target),
            source,
            target,
            edge_type,
            confidence,
            label: None,
            description: None,
            data_flow_kind: None,
            evidence: None,
        }
    }

    fn assert_local_analyzer(service: &AnalyzerServiceStatus) {
        assert_eq!(service.provider, AnalyzerProvider::Local);
        assert!(!service.billable);
        assert_eq!(service.credits_used, None);
    }

    #[test]
    fn analyzer_services_omit_optional_languages_without_files() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            Some("Ready".into()),
            Some(PythonAnalyzerStatus {
                mode: "auto".into(),
                status: "ty unavailable".into(),
                message: Some("missing ty".into()),
            }),
            Some(TypeScriptAnalyzerStatus {
                mode: "auto".into(),
                status: "language server unavailable".into(),
                message: Some("missing typescript-language-server".into()),
            }),
            Some(QmlAnalyzerStatus {
                mode: "auto".into(),
                status: "qmlls unavailable".into(),
                message: Some("missing qmlls".into()),
            }),
            AnalyzerFileCounts {
                rust: 1,
                ..AnalyzerFileCounts::default()
            },
            None,
        );

        assert!(services.iter().any(|service| service.id == "rust-analyzer"));
        assert!(!services
            .iter()
            .any(|service| service.kind == AnalyzerKind::Python));
        assert!(!services
            .iter()
            .any(|service| service.kind == AnalyzerKind::TypeScript));
        assert!(!services
            .iter()
            .any(|service| service.kind == AnalyzerKind::Qml));
    }

    #[test]
    fn analyzer_services_include_rust_typescript_and_qml_records() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            Some("Ready".into()),
            None,
            None,
            None,
            AnalyzerFileCounts {
                rust: 2,
                typescript: 3,
                python: 0,
                qml: 4,
            },
            Some("now".into()),
        );

        assert!(services.iter().any(|service| {
            service.id == "rust-analyzer"
                && service.kind == AnalyzerKind::Rust
                && service.engine == AnalyzerEngine::RustAnalyzer
        }));
        assert!(services
            .iter()
            .any(|service| service.id == "typescript-parser"));
        assert!(services.iter().any(|service| service.id == "qml-parser"));
        assert!(services.iter().all(|service| {
            service.provider == AnalyzerProvider::Local
                && !service.billable
                && service.credits_used.is_none()
        }));
    }

    #[test]
    fn analyzer_services_include_python_ty_record_when_ready() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            None,
            Some(PythonAnalyzerStatus {
                mode: "auto".into(),
                status: "ty ready".into(),
                message: None,
            }),
            None,
            None,
            AnalyzerFileCounts {
                python: 5,
                ..AnalyzerFileCounts::default()
            },
            None,
        );
        let ty = services
            .iter()
            .find(|service| service.id == "python-ty")
            .expect("python ty analyzer record");

        assert_eq!(ty.status, AnalyzerStatus::Ready);
        assert!(ty.capabilities.contains(&AnalyzerCapability::Diagnostics));
        assert_eq!(ty.files_indexed, 5);
        assert_local_analyzer(ty);
    }

    #[test]
    fn analyzer_services_report_python_parser_fallback() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            None,
            Some(PythonAnalyzerStatus {
                mode: "auto".into(),
                status: "ty unavailable".into(),
                message: Some("missing ty".into()),
            }),
            None,
            None,
            AnalyzerFileCounts {
                python: 7,
                ..AnalyzerFileCounts::default()
            },
            None,
        );

        assert!(services.iter().any(|service| {
            service.id == "python-ty" && service.status == AnalyzerStatus::Fallback
        }));
        assert!(services.iter().any(|service| {
            service.id == "python-parser"
                && service.status == AnalyzerStatus::Ready
                && service.capabilities == vec![AnalyzerCapability::Symbols]
        }));
    }

    #[test]
    fn analyzer_services_report_typescript_language_server_when_ready() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            None,
            None,
            Some(TypeScriptAnalyzerStatus {
                mode: "auto".into(),
                status: "language server ready".into(),
                message: None,
            }),
            None,
            AnalyzerFileCounts {
                typescript: 9,
                ..AnalyzerFileCounts::default()
            },
            None,
        );
        let service = services
            .iter()
            .find(|service| service.id == "typescript-language-server")
            .expect("typescript language server analyzer record");

        assert_eq!(service.status, AnalyzerStatus::Ready);
        assert_eq!(service.engine, AnalyzerEngine::TypeScriptLanguageServer);
        assert!(service
            .capabilities
            .contains(&AnalyzerCapability::References));
        assert!(!services
            .iter()
            .any(|service| service.id == "typescript-parser"));
    }

    #[test]
    fn analyzer_services_report_typescript_parser_fallback() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            None,
            None,
            Some(TypeScriptAnalyzerStatus {
                mode: "auto".into(),
                status: "language server unavailable".into(),
                message: Some("missing typescript-language-server".into()),
            }),
            None,
            AnalyzerFileCounts {
                typescript: 4,
                ..AnalyzerFileCounts::default()
            },
            None,
        );

        assert!(services.iter().any(|service| {
            service.id == "typescript-language-server" && service.status == AnalyzerStatus::Fallback
        }));
        assert!(services.iter().any(|service| {
            service.id == "typescript-parser"
                && service.status == AnalyzerStatus::Ready
                && service.message.as_deref().is_some_and(|message| {
                    message.contains("pnpm add -D typescript typescript-language-server")
                })
        }));
    }

    #[test]
    fn analyzer_services_report_qmlls_when_ready() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            None,
            None,
            None,
            Some(QmlAnalyzerStatus {
                mode: "auto".into(),
                status: "qmlls ready".into(),
                message: None,
            }),
            AnalyzerFileCounts {
                qml: 3,
                ..AnalyzerFileCounts::default()
            },
            None,
        );
        let service = services
            .iter()
            .find(|service| service.id == "qmlls")
            .expect("qmlls analyzer record");

        assert_eq!(service.status, AnalyzerStatus::Ready);
        assert_eq!(service.engine, AnalyzerEngine::QmlLanguageServer);
        assert!(service
            .capabilities
            .contains(&AnalyzerCapability::Diagnostics));
        assert!(!services.iter().any(|service| service.id == "qml-parser"));
    }

    #[test]
    fn analyzer_services_report_qml_parser_fallback() {
        let services = analyzer_services_from_counts(
            AnalyzerStatus::Ready,
            None,
            None,
            None,
            Some(QmlAnalyzerStatus {
                mode: "auto".into(),
                status: "qmlls unavailable".into(),
                message: Some("missing qmlls".into()),
            }),
            AnalyzerFileCounts {
                qml: 2,
                ..AnalyzerFileCounts::default()
            },
            None,
        );

        assert!(services
            .iter()
            .any(|service| service.id == "qmlls" && service.status == AnalyzerStatus::Fallback));
        assert!(services.iter().any(|service| {
            service.id == "qml-parser"
                && service.status == AnalyzerStatus::Fallback
                && service
                    .message
                    .as_deref()
                    .is_some_and(|message| message.contains("qmlls not found"))
        }));
    }

    #[test]
    fn readme_documents_analyzer_setup() {
        let readme =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
                .expect("workspace README");

        assert!(readme.contains("## Analyzer Setup"));
        assert!(readme.contains("rustup component add rust-analyzer"));
        assert!(readme.contains("uv tool install ty"));
        assert!(readme.contains("pnpm add -D typescript typescript-language-server"));
        assert!(readme.contains("--typescript-analyzer auto|parser|typescript-language-server"));
        assert!(readme.contains("--qml-analyzer auto|parser|qmlls"));
        assert!(readme.contains("--qmlls-no-cmake-calls"));
    }

    #[test]
    fn frontend_readme_documents_local_typescript_semantic_setup() {
        let readme = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/README.md"),
        )
        .expect("frontend README");

        assert!(readme.contains("## Optional TypeScript semantic analysis"));
        assert!(readme.contains("pnpm add -D typescript typescript-language-server"));
        assert!(readme.contains("parser"));
        assert!(readme.contains("node_modules/.bin/typescript-language-server"));
    }

    fn trace_snapshot() -> GraphSnapshot {
        let mut caller = test_node("useUsers", Some("frontend/useUsers.ts"), Some("frontend"));
        caller.id = "caller".into();
        caller.language = Some("typescript".into());
        caller.node_type = graph_core::NodeType::Hook;
        caller.reachability = Some(SourceReachability::Active);
        let mut endpoint = test_node("GET /api/users", Some("src/main.rs"), Some("crate root"));
        endpoint.id = "endpoint".into();
        endpoint.node_type = graph_core::NodeType::Endpoint;
        endpoint.reachability = Some(SourceReachability::Active);
        let mut handler = test_node("users", Some("src/main.rs"), Some("crate root"));
        handler.id = "handler".into();
        handler.reachability = Some(SourceReachability::Active);
        handler.signature = Some("async fn users() -> Json<Vec<User>>".into());
        let mut service = test_node("list_users", Some("src/service.rs"), Some("service"));
        service.id = "service".into();
        service.reachability = Some(SourceReachability::Active);
        let mut model = test_node("User", Some("src/model.rs"), Some("model"));
        model.id = "model".into();
        model.node_type = graph_core::NodeType::Struct;
        model.reachability = Some(SourceReachability::Active);
        let mut detached = test_node("GET /api/scratch", Some("src/scratch.rs"), Some("scratch"));
        detached.id = "detached-endpoint".into();
        detached.node_type = graph_core::NodeType::Endpoint;
        detached.reachability = Some(SourceReachability::Detached);

        let mut request = test_edge(
            EdgeType::DataFlow,
            "caller",
            "endpoint",
            EdgeConfidence::Semantic,
        );
        request.id = "request".into();
        request.data_flow_kind = Some(graph_core::DataFlowKind::ApiRequest);
        request.evidence = Some("fetch('/api/users')".into());
        let mut response = test_edge(
            EdgeType::DataFlow,
            "handler",
            "endpoint",
            EdgeConfidence::Semantic,
        );
        response.id = "response".into();
        response.data_flow_kind = Some(graph_core::DataFlowKind::ApiResponse);
        response.evidence = Some("Json<Vec<User>>".into());
        let mut model_flow = test_edge(
            EdgeType::DataFlow,
            "service",
            "model",
            EdgeConfidence::Semantic,
        );
        model_flow.id = "model-flow".into();
        model_flow.data_flow_kind = Some(graph_core::DataFlowKind::ModelUse);

        GraphSnapshot {
            nodes: vec![caller, endpoint, handler, service, model, detached],
            edges: vec![
                test_edge(
                    EdgeType::ApiCall,
                    "caller",
                    "endpoint",
                    EdgeConfidence::Semantic,
                ),
                request,
                test_edge(
                    EdgeType::EndpointHandler,
                    "endpoint",
                    "handler",
                    EdgeConfidence::Exact,
                ),
                test_edge(
                    EdgeType::Calls,
                    "handler",
                    "service",
                    EdgeConfidence::Semantic,
                ),
                response,
                model_flow,
            ],
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        }
    }

    #[test]
    fn route_trace_includes_api_endpoint_handler_and_response_steps() {
        let snapshot = trace_snapshot();
        let endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.id == "endpoint")
            .unwrap();
        let trace = build_route_trace(&snapshot, endpoint);
        let kinds = trace.steps.iter().map(|step| step.kind).collect::<Vec<_>>();
        assert!(kinds.contains(&TraceStepKind::ApiRequest));
        assert!(kinds.contains(&TraceStepKind::Endpoint));
        assert!(kinds.contains(&TraceStepKind::EndpointHandler));
        assert!(kinds.contains(&TraceStepKind::BackendHandler));
        assert!(kinds.contains(&TraceStepKind::ServiceCall));
        assert!(kinds.contains(&TraceStepKind::ApiResponse));
        assert_eq!(trace.route_key.as_deref(), Some("GET /api/users"));
    }

    #[test]
    fn detached_selected_node_returns_trace_warning() {
        let snapshot = trace_snapshot();
        let detached = snapshot
            .nodes
            .iter()
            .find(|node| node.id == "detached-endpoint")
            .unwrap();
        let trace = build_node_trace(&snapshot, detached);
        assert!(trace
            .warnings
            .iter()
            .any(|warning| warning.contains("detached")));
        assert!(trace
            .steps
            .iter()
            .any(|step| step.kind == TraceStepKind::DetachedSource));
    }

    #[test]
    fn route_trace_query_lookup_uses_method_and_path_key() {
        let snapshot = trace_snapshot();
        let key = graph_core::route_key("get", "/api/users").key;
        let endpoint = graph_query::find_active_endpoint_by_route_key(&snapshot, &key).unwrap();
        let trace = build_route_trace(&snapshot, endpoint);

        assert_eq!(trace.route_key.as_deref(), Some("GET /api/users"));
        assert!(trace.summary.contains("Route GET /api/users"));
    }

    #[test]
    fn ambiguous_active_route_trace_emits_warning() {
        let mut snapshot = trace_snapshot();
        let mut duplicate = snapshot
            .nodes
            .iter()
            .find(|node| node.id == "endpoint")
            .unwrap()
            .clone();
        duplicate.id = "endpoint-duplicate".into();
        duplicate.file = Some("src/other.rs".into());
        snapshot.nodes.push(duplicate);

        let endpoint = snapshot
            .nodes
            .iter()
            .find(|node| node.id == "endpoint")
            .unwrap();
        let trace = build_route_trace(&snapshot, endpoint);

        assert!(trace
            .warnings
            .iter()
            .any(|warning| warning.contains("Multiple active endpoint")));
    }

    #[test]
    fn trace_excludes_generated_neighbors_by_default() {
        let mut snapshot = trace_snapshot();
        let mut generated = test_node("generated", Some("target/out.rs"), Some("generated"));
        generated.id = "generated".into();
        generated.reachability = Some(SourceReachability::Generated);
        snapshot.nodes.push(generated);
        snapshot.edges.push(test_edge(
            EdgeType::Calls,
            "handler",
            "generated",
            EdgeConfidence::Semantic,
        ));
        let handler = snapshot
            .nodes
            .iter()
            .find(|node| node.id == "handler")
            .unwrap();

        let trace = build_node_trace(&snapshot, handler);

        assert!(!trace
            .steps
            .iter()
            .any(|step| step.node_id.as_deref() == Some("generated")));
    }

    #[test]
    fn reference_records_preserve_unresolved_source_locations() {
        let location = ReferenceRecord {
            node: None,
            location: SourceLocation {
                file: "src/lib.rs".into(),
                line: 7,
                character: 3,
                range: Some(LspRange {
                    start: LspPosition {
                        line: 6,
                        character: 3,
                    },
                    end: LspPosition {
                        line: 6,
                        character: 8,
                    },
                }),
            },
        };
        assert!(location.node.is_none());
        assert_eq!(location.location.line, 7);
        assert_eq!(location.location.range.unwrap().start.line, 6);
    }

    #[test]
    fn lsp_diagnostic_converts_and_associates_to_node() {
        let node = test_node("main", Some("src/main.rs"), Some("app"));
        let symbol_index = SymbolIndex::from_nodes(std::slice::from_ref(&node));
        let diagnostic: ra_client::LspDiagnostic = serde_json::from_value(serde_json::json!({
            "range": {
                "start": { "line": 0, "character": 1 },
                "end": { "line": 0, "character": 2 }
            },
            "severity": 1,
            "source": "rustc",
            "message": "broken"
        }))
        .unwrap();

        let record = diagnostic_from_lsp("src/main.rs", 0, diagnostic, &symbol_index);
        assert_eq!(record.severity, DiagnosticSeverity::Error);
        assert_eq!(record.source.as_deref(), Some("rustc"));
        assert_eq!(record.related_node_ids, vec![node.id]);
    }

    #[test]
    fn graph_patch_serializes_diagnostics_and_changed_files() {
        let patch = GraphPatch {
            diagnostics: vec![DiagnosticRecord {
                id: "diagnostic:src/main.rs:0:0:0".into(),
                language: LanguageId::Rust,
                file: "src/main.rs".into(),
                range: None,
                severity: DiagnosticSeverity::Warning,
                source: Some("rustc".into()),
                message: "careful".into(),
                code: None,
                related_node_ids: vec!["fn:main@1".into()],
            }],
            changed_files: vec!["src/main.rs".into()],
            ..GraphPatch::default()
        };
        let value = serde_json::to_value(&patch).unwrap();
        assert_eq!(value["changedFiles"][0], "src/main.rs");
        assert_eq!(value["diagnostics"][0]["message"], "careful");
    }

    fn temp_project_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("rust-watcher-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn missing_layout_file_is_not_an_error() {
        let root = temp_project_root("missing-layout");
        let layout = load_layout(&root).unwrap();
        assert!(layout.nodes.is_empty());
        let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_layout_file_is_renamed_and_ignored() {
        let root = temp_project_root("corrupt-layout");
        let path = layout_path(&root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not-json").unwrap();

        let layout = load_layout(&root).unwrap();

        assert!(layout.nodes.is_empty());
        assert!(!path.exists());
        let backups = std::fs::read_dir(storage_dir_for_project(&root))
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("layout.corrupt.")
            })
            .count();
        assert_eq!(backups, 1);
        let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn layout_save_load_roundtrip_and_clear() {
        let root = temp_project_root("layout-roundtrip");
        let layout = LayoutStore {
            nodes: HashMap::from([(
                "node:a".into(),
                LayoutNode {
                    node_id: "node:a".into(),
                    x: 12.0,
                    y: -8.0,
                    vx: 0.5,
                    vy: -0.25,
                    pinned: Some(true),
                    updated_at: "1".into(),
                },
            )]),
        };
        save_layout(&root, &layout).unwrap();
        let loaded = load_layout(&root).unwrap();
        assert_eq!(loaded.nodes["node:a"].x, 12.0);
        assert_eq!(loaded.nodes["node:a"].pinned, Some(true));
        clear_layout(&root).unwrap();
        assert!(load_layout(&root).unwrap().nodes.is_empty());
        let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn layout_applies_to_snapshot_and_ignores_stale_nodes() {
        let mut node = test_node("main", Some("src/main.rs"), Some("app"));
        node.id = "node:live".into();
        let mut snapshot = GraphSnapshot {
            nodes: vec![node],
            edges: Vec::new(),
            files: Vec::new(),
            events: Vec::new(),
            status: AppStatus::empty(),
        };
        let layout = LayoutStore {
            nodes: HashMap::from([
                (
                    "node:live".into(),
                    LayoutNode {
                        node_id: "node:live".into(),
                        x: 42.0,
                        y: 9.0,
                        vx: 0.0,
                        vy: 0.0,
                        pinned: Some(true),
                        updated_at: "1".into(),
                    },
                ),
                (
                    "node:stale".into(),
                    LayoutNode {
                        node_id: "node:stale".into(),
                        x: 1.0,
                        y: 1.0,
                        vx: 1.0,
                        vy: 1.0,
                        pinned: Some(true),
                        updated_at: "1".into(),
                    },
                ),
            ]),
        };
        apply_layout_store_to_snapshot(&mut snapshot, &layout);
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].x, 42.0);
        assert_eq!(snapshot.nodes[0].pinned, Some(true));
    }

    #[test]
    fn saved_views_roundtrip() {
        let root = temp_project_root("views-roundtrip");
        let views = SavedViewsStore {
            views: vec![SavedView {
                id: "view:1".into(),
                name: "Backend".into(),
                filters: serde_json::json!({ "languages": ["rust"] }),
                focused_node_id: Some("node:a".into()),
                collapsed_groups: vec!["file:src/main.rs".into()],
                layout_overrides: serde_json::json!({}),
                created_at: "1".into(),
                updated_at: "2".into(),
            }],
        };
        save_views(&root, &views).unwrap();
        let loaded = load_views(&root).unwrap();
        assert_eq!(loaded.views.len(), 1);
        assert_eq!(loaded.views[0].name, "Backend");
        assert_eq!(loaded.views[0].collapsed_groups, vec!["file:src/main.rs"]);
        let _ = std::fs::remove_dir_all(storage_dir_for_project(&root));
        let _ = std::fs::remove_dir_all(root);
    }
}
